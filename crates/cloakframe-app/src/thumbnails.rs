//! Photo and video thumbnails for the input grid, made one at a time on a worker thread for the cells on
//! screen. Only the most recently made are kept, so a folder of thousands of photos does not fill
//! the GPU's memory.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::media::decode::{frame_size, source_frame};
use cloakframe_engine::media::source;
use cloakframe_engine::photo::budget::MemoryBudget;
use cloakframe_engine::photo::decode::thumbnail;
use cloakframe_engine::scan::MediaKind;
use eframe::egui;

/// Pixels along the longer side: enough for a 128-point cell at twice the pixel density.
const SIDE: u32 = 256;
const KEPT: usize = 200;

type Made = (PathBuf, Option<(u32, u32, Vec<u8>)>);

/// FFmpeg and ffprobe, for the first frame of a video.
pub type VideoTools = (PathBuf, PathBuf);

pub struct Thumbnails {
    requests: mpsc::Sender<(PathBuf, MediaKind)>,
    made: mpsc::Receiver<Made>,
    cancel: CancelToken,
    asked: HashSet<PathBuf>,
    textures: HashMap<PathBuf, Option<egui::TextureHandle>>,
    order: VecDeque<PathBuf>,
}

impl Thumbnails {
    pub fn new(ctx: &egui::Context, video: Option<VideoTools>) -> std::io::Result<Self> {
        let (requests, incoming) = mpsc::channel::<(PathBuf, MediaKind)>();
        let (send, made) = mpsc::channel();
        let cancel = CancelToken::new();
        let (token, repaint) = (cancel.clone(), ctx.clone());
        std::thread::Builder::new()
            .name("cloakframe-thumbnails".into())
            .spawn(move || {
                let budget = MemoryBudget::for_this_computer();
                for (path, kind) in incoming {
                    if token.is_cancelled() {
                        return;
                    }
                    let picture = match kind {
                        MediaKind::Image => thumbnail(&path, SIDE, &budget, &token).ok(),
                        MediaKind::Video => video
                            .as_ref()
                            .and_then(|tools| first_frame(tools, &path, &token)),
                    };
                    if send.send((path, picture)).is_err() {
                        return;
                    }
                    repaint.request_repaint();
                }
            })?;
        Ok(Self {
            requests,
            made,
            cancel,
            asked: HashSet::new(),
            textures: HashMap::new(),
            order: VecDeque::new(),
        })
    }

    /// Uploads the thumbnails made since the last frame.
    pub fn take(&mut self, ctx: &egui::Context) {
        while let Ok(made) = self.made.try_recv() {
            self.made_now(made, ctx);
        }
    }

    fn made_now(&mut self, (path, picture): Made, ctx: &egui::Context) {
        let texture = picture.map(|(width, height, rgba)| {
            let image =
                egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
            ctx.load_texture(path.to_string_lossy(), image, egui::TextureOptions::LINEAR)
        });
        self.textures.insert(path.clone(), texture);
        self.order.push_back(path);
        while self.order.len() > KEPT {
            if let Some(old) = self.order.pop_front() {
                self.textures.remove(&old);
                self.asked.remove(&old);
            }
        }
    }

    /// Waits for every thumbnail asked for, for tests.
    pub fn wait(&mut self, ctx: &egui::Context) {
        while self.asked.iter().any(|p| !self.textures.contains_key(p)) {
            match self.made.recv() {
                Ok(made) => self.made_now(made, ctx),
                Err(_) => return,
            }
        }
    }

    /// The thumbnail of `path` if it is made; asks for it otherwise. `None` inside means the
    /// photo could not be read.
    pub fn get(&mut self, path: &Path, kind: MediaKind) -> Option<Option<&egui::TextureHandle>> {
        if let Some(texture) = self.textures.get(path) {
            return Some(texture.as_ref());
        }
        if self.asked.insert(path.to_path_buf()) {
            let _ = self.requests.send((path.to_path_buf(), kind));
        }
        None
    }
}

/// The video's first frame as the passes see it, in display orientation, as RGBA rows.
fn first_frame(
    (ffmpeg, ffprobe): &VideoTools,
    path: &Path,
    cancel: &CancelToken,
) -> Option<(u32, u32, Vec<u8>)> {
    let opened = source::open(ffprobe, path, cancel).ok()?;
    let size = frame_size(&opened.spec, Some(SIDE));
    let frame = source_frame(ffmpeg, path, &opened.spec, &opened.frames, 0, size, cancel).ok()?;
    let rgba = frame
        .data
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|&[r, g, b]| [r, g, b, 255])
        .collect();
    Some((frame.width, frame.height, rgba))
}

impl Drop for Thumbnails {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn photos_get_thumbnails_and_unreadable_ones_none() {
        let assets = std::env::var_os("CLOAKFRAME_TEST_ASSETS")
            .map(PathBuf::from)
            .expect("CLOAKFRAME_TEST_ASSETS is not set (see CONTRIBUTING.md)");
        let dir = std::env::temp_dir().join(format!("cloakframe-thumbs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let broken = dir.join("broken.jpg");
        std::fs::write(&broken, b"not a photo").unwrap();
        let photo = assets.join("lena.jpg");

        let ctx = egui::Context::default();
        let mut thumbnails = Thumbnails::new(&ctx, None).unwrap();
        assert!(thumbnails.get(&photo, MediaKind::Image).is_none());
        assert!(thumbnails.get(&broken, MediaKind::Image).is_none());
        assert!(
            thumbnails.get(&photo, MediaKind::Image).is_none(),
            "asked once only"
        );
        for _ in 0..2 {
            let made = thumbnails
                .made
                .recv_timeout(Duration::from_secs(30))
                .unwrap();
            thumbnails.made_now(made, &ctx);
        }
        let texture = thumbnails.get(&photo, MediaKind::Image).unwrap().unwrap();
        assert_eq!(texture.size_vec2().max_elem(), SIDE as f32);
        assert!(thumbnails.get(&broken, MediaKind::Image).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tool(name: &str) -> PathBuf {
        std::env::var_os(name)
            .map(PathBuf::from)
            .unwrap_or_else(|| panic!("{name} is not set (see CONTRIBUTING.md)"))
    }

    /// The first frame, at the thumbnail size in display orientation; none without FFmpeg.
    #[test]
    fn videos_get_their_first_frame() {
        let video = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/defects/P-3/rotated-90.mp4");
        let ctx = egui::Context::default();
        let tools = (tool("CLOAKFRAME_FFMPEG"), tool("CLOAKFRAME_FFPROBE"));
        for (tools, expected) in [(Some(tools), true), (None, false)] {
            let mut thumbnails = Thumbnails::new(&ctx, tools).unwrap();
            assert!(thumbnails.get(&video, MediaKind::Video).is_none());
            let made = thumbnails
                .made
                .recv_timeout(Duration::from_secs(60))
                .unwrap();
            thumbnails.made_now(made, &ctx);
            let texture = thumbnails.get(&video, MediaKind::Video).unwrap();
            assert_eq!(texture.is_some(), expected);
            if let Some(texture) = texture {
                let [width, height] = texture.size();
                assert_eq!(width.max(height), SIDE as usize);
                assert!(height > width, "turned upright: {width}x{height}");
            }
        }
    }
}
