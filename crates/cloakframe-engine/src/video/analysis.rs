//! A video's first pass (plan 4.4): every source frame decoded at analysis size, while the
//! inference thread detects the frame before, compared with the one before for scene cuts, and
//! its detections appended to the item's record. No frame is skipped.

use std::fmt;
use std::io;
use std::path::Path;
use std::sync::mpsc;
use std::thread;

use cloakframe_core::coverage::{CoverageEvidence, Problem};
use cloakframe_core::detection::{DetectionKind, FrameIndex, KindThresholds, Thresholds};
use cloakframe_core::geometry::Rect;
use cloakframe_core::review::Digest;
use cloakframe_core::tracking::scene::{SceneCutDetector, SceneCuts, Thumbnail};
use cloakframe_core::tracking::{Observation, TrackingWindows};
use sha2::{Digest as _, Sha256};

use super::record::{Detected, Order, RecordError, RecordWriter};
use crate::cancel::CancelToken;
use crate::detect::models::DetectionModels;
use crate::detect::{DetectError, DetectorHandle, Rgb8Image, tiles};
use crate::media::decode::{DecodeError, FrameReader, frame_size};
use crate::media::source::Source;

/// The long edge frames are analyzed at (1.12.1's).
pub const LONG_EDGE: u32 = 960;
/// Every detection down to this score is recorded: weaker ones carry a track through a frame the
/// detector was unsure of (P-2: high and low scores).
pub const DETECTION_FLOOR: f32 = 0.1;
/// Frames decoded ahead of detection. The decode at analysis size is several times faster than
/// detection, so a few are enough to keep the inference thread busy.
const FRAMES_AHEAD: usize = 4;

/// What decides a video's detections besides the source.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisSettings {
    pub models: DetectionModels,
    /// The run's masking thresholds, as photos have them.
    pub thresholds: Thresholds,
    /// Tile mode as in photos.
    pub tiles: bool,
    /// Always the defaults in a run. Shorter interpolation leaves holes the defaults fill, which
    /// the tests of the publish gate need, since the defaults leave no gap (docs/M5_RENDER.md).
    pub windows: TrackingWindows,
}

impl Default for AnalysisSettings {
    /// A photo's defaults.
    fn default() -> Self {
        let models = DetectionModels::default();
        Self {
            models,
            thresholds: models.default_thresholds(),
            tiles: true,
            windows: TrackingWindows::default(),
        }
    }
}

impl AnalysisSettings {
    pub fn kinds(&self) -> Vec<DetectionKind> {
        let mut kinds: Vec<DetectionKind> = self.models.ids().map(|id| id.kind()).collect();
        kinds.dedup();
        kinds
    }

    /// The masking thresholds' NMS with every score down to the floor.
    pub fn detection_thresholds(&self) -> Thresholds {
        let floor = |t: KindThresholds| {
            KindThresholds::new(DETECTION_FLOOR.min(t.score()), t.nms_iou())
                .expect("a threshold and the floor are valid")
        };
        Thresholds {
            face: floor(self.thresholds.face),
            plate: floor(self.thresholds.plate),
        }
    }

    /// The score that starts a track and counts as strong in post-processing: a little below the
    /// masking threshold, since a track gathers evidence over frames (1.12.1's
    /// `min(t, max(0.35, t − 0.1))`; 0.4 at the default 0.5).
    pub fn track_score(&self, kind: DetectionKind) -> f32 {
        let t = self.thresholds.of(kind).score();
        t.min(0.35f32.max(t - 0.1))
    }

    pub fn digest(&self) -> Digest {
        let mut h = Sha256::new();
        for kind in self.kinds() {
            let t = self.thresholds.of(kind);
            h.update(format!(
                "{kind:?} score {:08x} nms {:08x}\n",
                t.score().to_bits(),
                t.nms_iou().to_bits()
            ));
        }
        h.update(format!("tiles {}\n", self.tiles));
        let w = &self.windows;
        h.update(format!(
            "windows {:016x} {:016x} {:016x} {:016x} {:016x} {:016x}\n",
            w.max_lost.to_bits(),
            w.max_since_high.to_bits(),
            w.max_since_high_moving.to_bits(),
            w.max_interpolation.to_bits(),
            w.extension.to_bits(),
            w.max_cut_boundary.to_bits()
        ));
        Digest(h.finalize().into())
    }
}

#[derive(Debug)]
pub enum AnalysisError {
    Decode(DecodeError),
    /// The decode thread ended without saying why.
    DecoderGone,
    Detect(DetectError),
    Record(RecordError),
    Cancelled,
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(e) => e.fmt(f),
            Self::DecoderGone => f.write_str("the video decoder stopped"),
            Self::Detect(e) => e.fmt(f),
            Self::Record(e) => e.fmt(f),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for AnalysisError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Decode(e) => Some(e),
            Self::Detect(e) => Some(e),
            Self::Record(e) => Some(e),
            Self::DecoderGone | Self::Cancelled => None,
        }
    }
}

impl From<DetectError> for AnalysisError {
    fn from(e: DetectError) -> Self {
        Self::Detect(e)
    }
}

impl From<RecordError> for AnalysisError {
    fn from(e: RecordError) -> Self {
        Self::Record(e)
    }
}

impl From<io::Error> for AnalysisError {
    fn from(e: io::Error) -> Self {
        Self::Record(RecordError::Io(e))
    }
}

impl From<DecodeError> for AnalysisError {
    fn from(e: DecodeError) -> Self {
        match e {
            DecodeError::Cancelled => Self::Cancelled,
            e => Self::Decode(e),
        }
    }
}

/// What the first pass found besides the record.
#[derive(Debug)]
pub struct FirstPass {
    pub frames: u64,
    pub cuts: SceneCuts,
    /// Frames where a bounded model could not return every detection (D11).
    pub capped: Vec<CoverageEvidence>,
    /// A frame failed on the accelerator and was detected again on the CPU.
    pub reran_on_cpu: bool,
}

enum Decoded {
    Frame(Rgb8Image),
    /// Every listed frame was decoded.
    End,
    Failed(DecodeError),
}

fn decode(mut reader: FrameReader, frames: mpsc::SyncSender<Decoded>) {
    loop {
        let mut frame = Rgb8Image {
            width: 0,
            height: 0,
            data: Vec::new(),
        };
        let message = match reader.next_into(&mut frame) {
            Ok(true) => Decoded::Frame(frame),
            Ok(false) => Decoded::End,
            Err(e) => Decoded::Failed(e),
        };
        let last = !matches!(message, Decoded::Frame(_));
        // A send fails when the analysis stopped first, which then reports why; dropping the
        // reader ends FFmpeg.
        if frames.send(message).is_err() || last {
            return;
        }
    }
}

/// Runs the first pass of `source` into a new record at `record`. `progress` is told every frame
/// done.
#[allow(clippy::too_many_arguments)]
pub fn first_pass(
    ffmpeg: &Path,
    input: &Path,
    source: &Source,
    detector: &DetectorHandle,
    settings: &AnalysisSettings,
    record: &Path,
    cancel: &CancelToken,
    progress: impl FnMut(u64),
) -> Result<FirstPass, AnalysisError> {
    let size = frame_size(&source.spec, Some(LONG_EDGE));
    let reader = FrameReader::start(ffmpeg, input, &source.spec, &source.frames, size, cancel)?;
    let writer = RecordWriter::<Detected>::create(record, Order::Ascending)?;
    let (sender, frames) = mpsc::sync_channel(FRAMES_AHEAD);
    thread::scope(|s| {
        thread::Builder::new()
            .name("cloakframe-analysis-decode".into())
            .spawn_scoped(s, move || decode(reader, sender))?;
        // Takes the receiver, so the decoder stops once this returns.
        detect_frames(
            frames, writer, source, size, detector, settings, cancel, progress,
        )
    })
}

#[allow(clippy::too_many_arguments)]
fn detect_frames(
    frames: mpsc::Receiver<Decoded>,
    mut writer: RecordWriter<Detected>,
    source: &Source,
    (width, height): (u32, u32),
    detector: &DetectorHandle,
    settings: &AnalysisSettings,
    cancel: &CancelToken,
    mut progress: impl FnMut(u64),
) -> Result<FirstPass, AnalysisError> {
    let (display_width, display_height) = source.spec.display_size();
    let (sx, sy) = (
        display_width as f32 / width as f32,
        display_height as f32 / height as f32,
    );
    let thresholds = settings.detection_thresholds();
    let mut scenes = SceneCutDetector::default();
    let mut capped = Vec::new();
    let mut reran_on_cpu = false;
    let mut entries = Vec::new();
    let checkpoint = || {
        if cancel.is_cancelled() {
            Err(AnalysisError::Cancelled)
        } else {
            Ok(())
        }
    };
    let mut frame: FrameIndex = 0;
    // The frame queued to the detector and not yet recorded: queuing the next frame first keeps
    // the session busy while the next input is made (docs/M5_VERIFY.md).
    let mut queued: Option<tiles::Views> = None;
    loop {
        let next = match frames.recv() {
            Ok(Decoded::Frame(image)) => {
                scenes.push(Thumbnail::from_rgb(
                    image.width as usize,
                    image.height as usize,
                    &image.data,
                ));
                Some(tiles::submit_views(
                    detector,
                    image,
                    thresholds,
                    settings.tiles,
                    checkpoint,
                )?)
            }
            Ok(Decoded::End) => None,
            Ok(Decoded::Failed(e)) => return Err(e.into()),
            Err(mpsc::RecvError) => return Err(AnalysisError::DecoderGone),
        };
        let last = next.is_none();
        let Some(views) = std::mem::replace(&mut queued, next) else {
            if last {
                break;
            }
            continue;
        };
        let found = views.finish::<AnalysisError>(&settings.thresholds)?;
        reran_on_cpu |= found.reran_on_cpu;
        for &(model, dropped) in found.over_limit.iter().filter(|(_, d)| *d > 0) {
            let kept = found
                .detections
                .iter()
                .filter(|d| d.model() == model)
                .count() as u32;
            capped.push(CoverageEvidence::unresolved(Problem::CappedCandidates {
                model,
                frame: Some(frame),
                kept,
                found: kept + dropped,
            }));
        }
        entries.clear();
        for d in &found.detections {
            let b = d.bbox();
            let bbox = Rect::new(b.x() * sx, b.y() * sy, b.width() * sx, b.height() * sy)
                .expect("a detection's box scaled up");
            entries.push(Detected {
                model: d.model(),
                observation: Observation {
                    bbox,
                    score: d.score(),
                    roll: d.roll(),
                },
            });
        }
        writer.frame(frame, &entries)?;
        frame += 1;
        progress(frame);
        if last {
            break;
        }
    }
    let cuts = scenes.finish();
    writer.finish(cuts.frames())?;
    Ok(FirstPass {
        frames: frame,
        cuts,
        capped,
        reran_on_cpu,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stored review is bound to the settings' digest, so each setting that changes the
    /// tracks changes it.
    #[test]
    fn every_setting_changes_the_digest() {
        let base = AnalysisSettings::default();
        let changed = [
            AnalysisSettings {
                tiles: false,
                ..base.clone()
            },
            AnalysisSettings {
                thresholds: Thresholds {
                    face: KindThresholds::new(0.6, 0.4).unwrap(),
                    ..base.thresholds
                },
                ..base.clone()
            },
            AnalysisSettings {
                windows: TrackingWindows {
                    max_interpolation: 1.0,
                    ..TrackingWindows::default()
                },
                ..base.clone()
            },
        ];
        for settings in changed {
            assert_ne!(settings.digest(), base.digest(), "{settings:?}");
        }
        assert_eq!(AnalysisSettings::default().digest(), base.digest());
    }
}
