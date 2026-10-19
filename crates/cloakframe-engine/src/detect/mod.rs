pub mod models;
pub mod plate;
pub mod resize;
pub mod runtime;
pub mod yolo5face;
pub mod yunet;

use std::fmt;
use std::sync::mpsc;
use std::thread;

use cloakframe_core::detection::{Detection, ModelId, Thresholds};

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
    /// The model's inputs or outputs are not what the detector decodes. Not a reason to retry on
    /// another execution provider (plan 3).
    UnexpectedModel(String),
    /// The inference thread is gone (its detector panicked).
    Unavailable,
}

impl fmt::Display for DetectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed(reason) => write!(f, "detection failed: {reason}"),
            Self::UnexpectedModel(reason) => write!(f, "unexpected model: {reason}"),
            Self::Unavailable => f.write_str("inference thread is not running"),
        }
    }
}

impl std::error::Error for DetectError {}

/// What the detectors found in one image.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Found {
    pub detections: Vec<Detection>,
    /// By model, detections NMS kept that a model with a bounded output could not return. The
    /// caller records each as `Problem::CappedCandidates`, the only kind of omission D11 counts.
    /// The pinned models return every survivor (the plate graph's limit is lifted), so this is
    /// empty for them.
    pub over_limit: Vec<(ModelId, u32)>,
}

/// A loaded model session. `Send` but not required to be `Sync`: the inference thread is its
/// only owner, so no two callers can run one session at the same time (D8).
pub trait Detector: Send {
    fn detect(&mut self, image: &Rgb8Image, thresholds: &Thresholds) -> Result<Found, DetectError>;
}

/// Finds nothing. For runs that do not detect, and for tests.
pub struct NoDetections;

impl Detector for NoDetections {
    fn detect(&mut self, _: &Rgb8Image, _: &Thresholds) -> Result<Found, DetectError> {
        Ok(Found::default())
    }
}

type Request = (
    Rgb8Image,
    Thresholds,
    mpsc::SyncSender<Result<Found, DetectError>>,
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
                for (image, thresholds, reply) in incoming {
                    let _ = reply.send(detector.detect(&image, &thresholds));
                }
            })?;
        Ok(Self { requests })
    }

    pub fn detect(&self, image: Rgb8Image, thresholds: Thresholds) -> Result<Found, DetectError> {
        let (reply, response) = mpsc::sync_channel(1);
        self.requests
            .send((image, thresholds, reply))
            .map_err(|_| DetectError::Unavailable)?;
        response.recv().map_err(|_| DetectError::Unavailable)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloakframe_core::geometry::Rect;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct OneFacePerImage {
        busy: Arc<AtomicBool>,
    }

    impl Detector for OneFacePerImage {
        fn detect(&mut self, image: &Rgb8Image, _: &Thresholds) -> Result<Found, DetectError> {
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
            Ok(Found {
                detections: vec![face],
                ..Found::default()
            })
        }
    }

    fn image(width: u32) -> Rgb8Image {
        Rgb8Image {
            width,
            height: 2,
            data: vec![0; width as usize * 6],
        }
    }

    fn thresholds() -> Thresholds {
        models::DetectionModels::default().default_thresholds()
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
                        let found = handle.detect(image(w), thresholds()).unwrap();
                        assert_eq!(found.detections[0].bbox().width(), w as f32);
                    }
                });
            }
        });
    }

    #[test]
    fn detector_errors_reach_the_caller_and_the_thread_keeps_serving() {
        let handle = spawn();
        assert_eq!(
            handle.detect(image(0), thresholds()),
            Err(DetectError::Failed("empty image".into()))
        );
        assert!(handle.detect(image(4), thresholds()).is_ok());
    }

    #[test]
    fn a_panicking_detector_makes_the_handle_unavailable() {
        let handle = spawn();
        let t = thresholds();
        assert_eq!(handle.detect(image(13), t), Err(DetectError::Unavailable));
        assert_eq!(handle.detect(image(4), t), Err(DetectError::Unavailable));
    }
}
