//! A video under review (plan 6 "영상 검토"): its preview, opened on a thread of its own, which
//! decodes the frame on screen at the display size, masks it, or plays on from it smaller in
//! real time (plan 4.4 "미리보기").

use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use cloakframe_core::review::VideoEdits;
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::Rgb8Image;
use cloakframe_engine::media_processor::MediaProcessor;
use cloakframe_engine::review::stored_analysis;
use cloakframe_engine::session::SessionItem;
use cloakframe_engine::video::preview::{Playback, Preview, PreviewError};
use eframe::egui;

/// Playback's long edge, the size the first pass analyzes at: a 1080p upload takes under 6 ms
/// (docs/M0_EGUI.md), so decoding is what keeps up or not.
pub const PLAYBACK_EDGE: u32 = 960;

/// While decoding falls behind, a late frame is still shown once this long has passed without
/// one, so the picture keeps moving.
const LONGEST_WITHOUT_A_FRAME: Duration = Duration::from_millis(250);

enum Request {
    Frame(u64),
    Masked(u64, VideoEdits),
    Play {
        from: u64,
        edits: Option<VideoEdits>,
    },
    Stop,
}

enum Made {
    Opened(Arc<Preview>),
    OpenFailed(String),
    Frame {
        k: u64,
        masked: bool,
        playing: bool,
        image: egui::ColorImage,
    },
    FrameFailed(String),
    /// Playback reached the last frame, or stopped on a failure.
    Ended(Option<String>),
}

#[derive(Debug, Clone, PartialEq)]
enum Asked {
    Frame(u64),
    Masked(u64, VideoEdits),
}

pub struct Shown {
    pub frame: u64,
    pub masked: bool,
    pub texture: egui::TextureHandle,
}

pub struct Clip {
    pub preview: Option<Arc<Preview>>,
    /// Why the video cannot be shown.
    pub error: Option<String>,
    /// Why the last frame asked for, or playback, failed.
    pub frame_error: Option<String>,
    pub shown: Option<Shown>,
    pub playing: bool,
    asked: Option<Asked>,
    /// The edits the last masked frame was asked under.
    masked_with: Option<VideoEdits>,
    requests: mpsc::Sender<Request>,
    made: mpsc::Receiver<Made>,
    cancel: CancelToken,
}

impl Clip {
    pub fn open(processor: Arc<MediaProcessor>, item: &SessionItem, ctx: &egui::Context) -> Self {
        let (requests, incoming) = mpsc::channel();
        let (send, made) = mpsc::channel();
        let cancel = CancelToken::new();
        let (id, input, analysis) = (item.item, item.input.clone(), stored_analysis(item));
        let (token, repaint) = (cancel.clone(), ctx.clone());
        let spawned = std::thread::Builder::new()
            .name("cloakframe-app-video".into())
            .spawn(move || {
                let opened = processor.video().map_err(|e| e.to_string()).and_then(|v| {
                    v.preview(id, &input, &analysis, &token)
                        .map_err(|e| e.to_string())
                });
                let preview = match opened {
                    Ok(preview) => Arc::new(preview),
                    Err(e) => {
                        let _ = send.send(Made::OpenFailed(e));
                        repaint.request_repaint();
                        return;
                    }
                };
                let _ = send.send(Made::Opened(Arc::clone(&preview)));
                repaint.request_repaint();
                work(&preview, &incoming, &send, &repaint, &token);
            });
        let error = spawned.err().map(|e| e.to_string());
        Self {
            preview: None,
            error,
            frame_error: None,
            shown: None,
            playing: false,
            asked: None,
            masked_with: None,
            requests,
            made,
            cancel,
        }
    }

    /// Takes what the worker made since the last frame.
    pub fn take(&mut self, ctx: &egui::Context) {
        while let Ok(made) = self.made.try_recv() {
            self.made_now(made, ctx);
        }
    }

    /// Waits until the video is open and the frame asked for is on screen, for tests.
    pub fn wait(&mut self, ctx: &egui::Context) {
        while (self.preview.is_none() && self.error.is_none()) || self.asked.is_some() {
            match self.made.recv() {
                Ok(made) => self.made_now(made, ctx),
                Err(_) => return,
            }
        }
    }

    /// Waits for the next frame playback shows, for tests; `false` once it has ended.
    pub fn wait_for_playback(&mut self, ctx: &egui::Context) -> bool {
        while self.playing {
            let Ok(made) = self.made.recv() else {
                return false;
            };
            let frame = matches!(made, Made::Frame { playing: true, .. });
            self.made_now(made, ctx);
            if frame {
                return true;
            }
        }
        false
    }

    fn made_now(&mut self, made: Made, ctx: &egui::Context) {
        match made {
            Made::Opened(preview) => self.preview = Some(preview),
            Made::OpenFailed(e) => self.error = Some(e),
            Made::Frame {
                k,
                masked,
                playing,
                image,
            } => {
                if playing != self.playing {
                    return;
                }
                if !playing {
                    let answers = match &self.asked {
                        Some(Asked::Frame(f)) => *f == k && !masked,
                        Some(Asked::Masked(f, _)) => *f == k && masked,
                        None => false,
                    };
                    if !answers {
                        return;
                    }
                    self.asked = None;
                }
                self.frame_error = None;
                let options = egui::TextureOptions::LINEAR;
                match &mut self.shown {
                    Some(shown) => {
                        shown.texture.set(image, options);
                        shown.frame = k;
                        shown.masked = masked;
                    }
                    None => {
                        self.shown = Some(Shown {
                            frame: k,
                            masked,
                            texture: ctx.load_texture("video-frame", image, options),
                        });
                    }
                }
            }
            Made::FrameFailed(e) => {
                self.asked = None;
                self.frame_error = Some(e);
            }
            Made::Ended(failure) => {
                self.playing = false;
                if failure.is_some() {
                    self.frame_error = failure;
                }
            }
        }
    }

    /// Shows frame `k` as decoded, unless it is already shown or on its way.
    pub fn show(&mut self, k: u64) {
        let asked = Asked::Frame(k);
        let shown = self
            .shown
            .as_ref()
            .is_some_and(|s| s.frame == k && !s.masked);
        if self.playing || self.asked.as_ref() == Some(&asked) || (self.asked.is_none() && shown) {
            return;
        }
        self.asked = Some(asked);
        let _ = self.requests.send(Request::Frame(k));
    }

    /// Shows frame `k` masked under `edits`, unless it is already on its way.
    pub fn show_masked(&mut self, k: u64, edits: &VideoEdits) {
        let asked = Asked::Masked(k, edits.clone());
        if self.playing || self.asked.as_ref() == Some(&asked) {
            return;
        }
        let shown = self
            .shown
            .as_ref()
            .is_some_and(|s| s.frame == k && s.masked);
        if shown && self.asked.is_none() && self.masked_with.as_ref() == Some(edits) {
            return;
        }
        self.masked_with = Some(edits.clone());
        self.asked = Some(asked);
        let _ = self.requests.send(Request::Masked(k, edits.clone()));
    }

    /// Plays on from frame `from`, masked under `edits` if given.
    pub fn play(&mut self, from: u64, edits: Option<VideoEdits>) {
        self.playing = true;
        self.asked = None;
        self.frame_error = None;
        let _ = self.requests.send(Request::Play { from, edits });
    }

    pub fn stop(&mut self) {
        if self.playing {
            self.playing = false;
            let _ = self.requests.send(Request::Stop);
        }
    }

    /// The frame on screen while playing, or the one asked for.
    pub fn is_waiting(&self) -> bool {
        self.asked.is_some()
    }
}

impl Drop for Clip {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

fn color_image(frame: &Rgb8Image) -> egui::ColorImage {
    egui::ColorImage::from_rgb([frame.width as usize, frame.height as usize], &frame.data)
}

/// Answers requests until the clip is dropped. A newer request replaces the ones still waiting.
fn work(
    preview: &Preview,
    requests: &mpsc::Receiver<Request>,
    made: &mpsc::Sender<Made>,
    repaint: &egui::Context,
    cancel: &CancelToken,
) {
    let mut next = None;
    loop {
        let request = match next.take() {
            Some(r) => r,
            None => match requests.recv() {
                Ok(r) => r,
                Err(_) => return,
            },
        };
        let request = requests.try_iter().last().unwrap_or(request);
        let answer = match request {
            Request::Frame(k) => Some(preview.frame(k, None, cancel).map(|f| (k, false, f))),
            Request::Masked(k, edits) => Some(
                preview
                    .masked(k, &edits, None, cancel)
                    .map(|f| (k, true, f)),
            ),
            Request::Play { from, edits } => {
                let (ended, interrupted) =
                    play(preview, from, edits, requests, made, repaint, cancel);
                next = interrupted;
                if let Some(ended) = ended {
                    let _ = made.send(Made::Ended(ended.err().map(|e| e.to_string())));
                }
                None
            }
            Request::Stop => None,
        };
        let sent = match answer {
            Some(Ok((k, masked, frame))) => made.send(Made::Frame {
                k,
                masked,
                playing: false,
                image: color_image(&frame),
            }),
            Some(Err(e)) => made.send(Made::FrameFailed(e.to_string())),
            None => Ok(()),
        };
        if sent.is_err() {
            return;
        }
        repaint.request_repaint();
    }
}

/// Plays frames from `from` at the times they show, leaving out those already late unless none
/// has shown for a while. Returns how playback ended, `None` if a request stopped it, and that
/// request.
fn play(
    preview: &Preview,
    from: u64,
    edits: Option<VideoEdits>,
    requests: &mpsc::Receiver<Request>,
    made: &mpsc::Sender<Made>,
    repaint: &egui::Context,
    cancel: &CancelToken,
) -> (Option<Result<(), PreviewError>>, Option<Request>) {
    let mut playback: Playback<'_> =
        match preview.play(from, edits.as_ref(), Some(PLAYBACK_EDGE), cancel) {
            Ok(p) => p,
            Err(e) => return (Some(Err(e)), None),
        };
    let started = Instant::now();
    let start = preview.seconds(from);
    let due = |k: u64| started + Duration::from_secs_f64((preview.seconds(k) - start).max(0.0));
    let mut last_shown: Option<Instant> = None;
    loop {
        match requests.try_recv() {
            Ok(r) => return (None, Some(r)),
            Err(mpsc::TryRecvError::Disconnected) => return (None, None),
            Err(mpsc::TryRecvError::Empty) => {}
        }
        let (k, frame) = match playback.next_frame() {
            Ok(Some(next)) => next,
            Ok(None) => return (Some(Ok(())), None),
            Err(e) => return (Some(Err(e)), None),
        };
        let now = Instant::now();
        if let Some(wait) = due(k).checked_duration_since(now) {
            match requests.recv_timeout(wait) {
                Ok(r) => return (None, Some(r)),
                Err(mpsc::RecvTimeoutError::Disconnected) => return (None, None),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        } else if k + 1 < preview.frames()
            && now > due(k + 1)
            && last_shown.is_some_and(|t| now - t < LONGEST_WITHOUT_A_FRAME)
        {
            continue;
        }
        last_shown = Some(Instant::now());
        let sent = made.send(Made::Frame {
            k,
            masked: edits.is_some(),
            playing: true,
            image: color_image(&frame),
        });
        if sent.is_err() {
            return (None, None);
        }
        repaint.request_repaint();
    }
}
