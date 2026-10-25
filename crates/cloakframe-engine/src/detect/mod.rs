pub mod fallback;
pub mod models;
pub mod plate;
pub mod resize;
pub mod runtime;
pub mod tiles;
pub mod yolo5face;
pub mod yunet;

use std::any::Any;
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
    /// The inference thread is gone (its detector or input preparer panicked).
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
    /// The image failed on the accelerator and was analyzed again on the CPU.
    pub reran_on_cpu: bool,
}

/// The model input a detector's [`Preparer`] made from one image.
pub type Prepared = Box<dyn Any + Send>;

/// Makes a detector's model input from an image without its session.
pub type Preparer = Box<dyn Fn(&Rgb8Image) -> Prepared + Send>;

/// A loaded model session. `Send` but not required to be `Sync`: the inference thread is its
/// only owner, so no two callers can run one session at the same time (D8).
pub trait Detector: Send {
    fn detect(&mut self, image: &Rgb8Image, thresholds: &Thresholds) -> Result<Found, DetectError>;

    /// The work on an image that needs no session, which [`DetectorHandle`] does on its own
    /// thread while the session runs the image before. On an accelerator the session waits for
    /// the device, and the input made in line with it took a tenth of each frame
    /// (docs/M5_VERIFY.md).
    fn preparer(&self) -> Option<Preparer> {
        None
    }

    /// [`detect`](Self::detect) given what the preparer made from `image`.
    fn detect_prepared(
        &mut self,
        image: &Rgb8Image,
        _prepared: Prepared,
        thresholds: &Thresholds,
    ) -> Result<Found, DetectError> {
        self.detect(image, thresholds)
    }
}

/// `prepare`'s input for `image`, or nothing for an empty image, in which a detector finds
/// nothing without running its session.
fn input_unless_empty<T: Send + 'static>(
    image: &Rgb8Image,
    prepare: fn(&Rgb8Image) -> T,
) -> Prepared {
    if image.width == 0 || image.height == 0 {
        Box::new(())
    } else {
        Box::new(prepare(image))
    }
}

/// The input a detector's own preparer made. Another type is a detector wired to the wrong
/// preparer, a bug.
fn prepared<T: 'static>(prepared: Prepared) -> T {
    *prepared
        .downcast::<T>()
        .expect("model input from another detector's preparer")
}

/// Finds nothing. For runs that do not detect, and for tests.
pub struct NoDetections;

impl Detector for NoDetections {
    fn detect(&mut self, _: &Rgb8Image, _: &Thresholds) -> Result<Found, DetectError> {
        Ok(Found::default())
    }
}

type Reply = mpsc::SyncSender<Result<Found, DetectError>>;
type Request = (Rgb8Image, Thresholds, Reply);

/// Cloneable access to the detector running on its own thread. The thread ends when the last
/// handle is dropped.
#[derive(Debug, Clone)]
pub struct DetectorHandle {
    requests: mpsc::SyncSender<Request>,
}

impl DetectorHandle {
    pub fn spawn(mut detector: Box<dyn Detector>) -> std::io::Result<Self> {
        let (requests, incoming) = mpsc::sync_channel::<Request>(4);
        let Some(prepare) = detector.preparer() else {
            thread::Builder::new()
                .name("cloakframe-inference".into())
                .spawn(move || {
                    for (image, thresholds, reply) in incoming {
                        let _ = reply.send(detector.detect(&image, &thresholds));
                    }
                })?;
            return Ok(Self { requests });
        };
        // Inputs made ahead: while the session runs one, the next waits in the channel and the
        // preparer holds one more until there is room, so the next is ready when the session is
        // free.
        let (ready, inputs) = mpsc::sync_channel::<(Rgb8Image, Prepared, Thresholds, Reply)>(1);
        thread::Builder::new()
            .name("cloakframe-inference".into())
            .spawn(move || {
                for (image, input, thresholds, reply) in inputs {
                    let _ = reply.send(detector.detect_prepared(&image, input, &thresholds));
                }
            })?;
        thread::Builder::new()
            .name("cloakframe-prepare".into())
            .spawn(move || {
                for (image, thresholds, reply) in incoming {
                    let input = prepare(&image);
                    // Fails when the inference thread is gone; dropping the replies tells the
                    // callers.
                    if ready.send((image, input, thresholds, reply)).is_err() {
                        return;
                    }
                }
            })?;
        Ok(Self { requests })
    }

    pub fn detect(&self, image: Rgb8Image, thresholds: Thresholds) -> Result<Found, DetectError> {
        self.submit(image, thresholds)?.wait()
    }

    /// Queues `image` and returns at once, so the caller can queue the next image before taking
    /// this one's result. Images are detected in the order they are queued.
    pub fn submit(&self, image: Rgb8Image, thresholds: Thresholds) -> Result<Pending, DetectError> {
        let (reply, response) = mpsc::sync_channel(1);
        self.requests
            .send((image, thresholds, reply))
            .map_err(|_| DetectError::Unavailable)?;
        Ok(Pending(response))
    }
}

/// The result of a queued image, see [`DetectorHandle::submit`].
#[derive(Debug)]
pub struct Pending(mpsc::Receiver<Result<Found, DetectError>>);

impl Pending {
    pub fn wait(self) -> Result<Found, DetectError> {
        self.0.recv().map_err(|_| DetectError::Unavailable)?
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

    /// Its input is the image's width and the name of the thread that made it; the face it
    /// finds is as wide as the input says.
    struct MadeAhead;

    impl Detector for MadeAhead {
        fn detect(&mut self, _: &Rgb8Image, _: &Thresholds) -> Result<Found, DetectError> {
            unreachable!("the handle detects what the preparer made")
        }

        fn preparer(&self) -> Option<Preparer> {
            Some(Box::new(|image| {
                let thread = thread::current().name().map(str::to_owned);
                Box::new((image.width, thread))
            }))
        }

        fn detect_prepared(
            &mut self,
            image: &Rgb8Image,
            input: Prepared,
            _: &Thresholds,
        ) -> Result<Found, DetectError> {
            let (width, thread) = prepared::<(u32, Option<String>)>(input);
            assert_eq!(thread.as_deref(), Some("cloakframe-prepare"));
            assert_ne!(width, 13, "detector bug");
            let bbox = Rect::new(0.0, 0.0, width as f32, image.height as f32).unwrap();
            let face = Detection::new(ModelId::YuNet, 0.9, bbox, None, None).unwrap();
            Ok(Found {
                detections: vec![face],
                ..Found::default()
            })
        }
    }

    #[test]
    fn inputs_are_made_on_their_own_thread_and_queued_images_keep_their_order() {
        let handle = DetectorHandle::spawn(Box::new(MadeAhead)).unwrap();
        let queued: Vec<Pending> = (1..=6)
            .map(|w| handle.submit(image(w), thresholds()).unwrap())
            .collect();
        for (w, pending) in (1..=6).zip(queued) {
            let found = pending.wait().unwrap();
            assert_eq!(found.detections[0].bbox().width(), w as f32);
        }
        assert_eq!(
            handle.detect(image(13), thresholds()),
            Err(DetectError::Unavailable)
        );
        assert_eq!(
            handle.detect(image(4), thresholds()),
            Err(DetectError::Unavailable)
        );
    }
}
