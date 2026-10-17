use std::fmt;
use std::sync::mpsc;
use std::thread;

use cloakframe_core::detection::Detection;

/// sRGB RGB8 pixels in display orientation, the detection input of plan 4.3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgb8Image {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectError {
    Failed(String),
    /// The inference thread is gone (its detector panicked).
    Unavailable,
}

impl fmt::Display for DetectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed(reason) => write!(f, "detection failed: {reason}"),
            Self::Unavailable => f.write_str("inference thread is not running"),
        }
    }
}

impl std::error::Error for DetectError {}

/// A loaded model session. `Send` but not required to be `Sync`: the inference thread is its
/// only owner, so no two callers can run one session at the same time (D8).
pub trait Detector: Send {
    fn detect(&mut self, image: &Rgb8Image) -> Result<Vec<Detection>, DetectError>;
}

type Request = (
    Rgb8Image,
    mpsc::SyncSender<Result<Vec<Detection>, DetectError>>,
);

/// Cloneable access to the detector running on its own thread. The thread ends when the last
/// handle is dropped.
#[derive(Debug, Clone)]
pub struct DetectorHandle {
    requests: mpsc::SyncSender<Request>,
}

impl DetectorHandle {
    pub fn spawn(mut detector: Box<dyn Detector>) -> std::io::Result<Self> {
        let (requests, incoming) = mpsc::sync_channel::<Request>(4);
        thread::Builder::new()
            .name("cloakframe-inference".into())
            .spawn(move || {
                for (image, reply) in incoming {
                    let _ = reply.send(detector.detect(&image));
                }
            })?;
        Ok(Self { requests })
    }

    pub fn detect(&self, image: Rgb8Image) -> Result<Vec<Detection>, DetectError> {
        let (reply, response) = mpsc::sync_channel(1);
        self.requests
            .send((image, reply))
            .map_err(|_| DetectError::Unavailable)?;
        response.recv().map_err(|_| DetectError::Unavailable)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloakframe_core::detection::ModelId;
    use cloakframe_core::geometry::Rect;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct OneFacePerImage {
        busy: Arc<AtomicBool>,
    }

    impl Detector for OneFacePerImage {
        fn detect(&mut self, image: &Rgb8Image) -> Result<Vec<Detection>, DetectError> {
            assert!(!self.busy.swap(true, Ordering::SeqCst), "concurrent detect");
            if image.width == 0 {
                self.busy.store(false, Ordering::SeqCst);
                return Err(DetectError::Failed("empty image".into()));
            }
            assert_ne!(image.width, 13, "detector bug");
            thread::sleep(std::time::Duration::from_millis(1));
            let bbox = Rect::new(0.0, 0.0, image.width as f32, image.height as f32).unwrap();
            let face = Detection::new(ModelId::YuNet, 0.9, bbox, None, None).unwrap();
            self.busy.store(false, Ordering::SeqCst);
            Ok(vec![face])
        }
    }

    fn image(width: u32) -> Rgb8Image {
        Rgb8Image {
            width,
            height: 2,
            data: vec![0; width as usize * 6],
        }
    }

    fn spawn() -> DetectorHandle {
        DetectorHandle::spawn(Box::new(OneFacePerImage {
            busy: Arc::new(AtomicBool::new(false)),
        }))
        .unwrap()
    }

    #[test]
    fn serializes_calls_from_many_threads() {
        let handle = spawn();
        thread::scope(|s| {
            for w in 1..=8 {
                let handle = handle.clone();
                s.spawn(move || {
                    for _ in 0..10 {
                        let found = handle.detect(image(w)).unwrap();
                        assert_eq!(found[0].bbox().width(), w as f32);
                    }
                });
            }
        });
    }

    #[test]
    fn detector_errors_reach_the_caller_and_the_thread_keeps_serving() {
        let handle = spawn();
        assert_eq!(
            handle.detect(image(0)),
            Err(DetectError::Failed("empty image".into()))
        );
        assert!(handle.detect(image(4)).is_ok());
    }

    #[test]
    fn a_panicking_detector_makes_the_handle_unavailable() {
        let handle = spawn();
        assert_eq!(handle.detect(image(13)), Err(DetectError::Unavailable));
        assert_eq!(handle.detect(image(4)), Err(DetectError::Unavailable));
    }
}
