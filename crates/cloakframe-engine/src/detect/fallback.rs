//! Each model on the platform's accelerator with the CPU behind it (plan 3). A model whose session
//! cannot be made on the accelerator starts on the CPU. When an image fails on the accelerator it
//! is analyzed again on a CPU session, which runs that model for the rest of the run. Failures on
//! the CPU and model errors are not retried.

use std::sync::{Arc, Mutex};

use cloakframe_core::detection::{ModelId, Thresholds};

use super::plate::Plate;
use super::runtime::{Backend, OrtRuntime};
use super::yolo5face::Yolo5Face;
use super::yunet::YuNet;
use super::{DetectError, Detector, Found, Prepared, Preparer, Rgb8Image};

/// Whether a run may use the accelerator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Acceleration {
    #[default]
    Gpu,
    CpuOnly,
}

/// Where `model` runs: the runtime's `accelerator`, except that under CoreML the plate model stays
/// on the CPU, since CoreML rejects the plate graph's empty tensor whenever nothing is found
/// (docs/M0_ORT_SPIKE.md, still so in ORT 1.30 with the graph's NMS lifted).
pub fn backend_for(
    model: ModelId,
    acceleration: Acceleration,
    accelerator: Option<Backend>,
) -> Backend {
    match (acceleration, accelerator, model) {
        (Acceleration::CpuOnly, _, _)
        | (_, None, _)
        | (_, Some(Backend::CoreMl), ModelId::PlateYolov9T) => Backend::Cpu,
        (_, Some(accelerator), _) => accelerator,
    }
}

/// Why a model left its accelerator for the CPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fallback {
    pub from: Backend,
    pub reason: String,
    /// The session could not be made there, so no image ran on it.
    pub at_start: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelBackend {
    pub model: ModelId,
    pub backend: Backend,
    pub fallback: Option<Fallback>,
}

#[derive(Debug, Default)]
struct Record {
    models: Vec<ModelBackend>,
    sessions: usize,
}

/// Where each model of a run runs, and how many sessions the run made, readable while and after
/// it runs.
#[derive(Debug, Clone, Default)]
pub struct Backends(Arc<Mutex<Record>>);

impl Backends {
    pub fn get(&self) -> Vec<ModelBackend> {
        self.lock().models.clone()
    }

    /// Every session made for the run's models, on any backend (D7).
    pub fn sessions_made(&self) -> usize {
        self.lock().sessions
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Record> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn add(&self, entry: ModelBackend) -> usize {
        let mut record = self.lock();
        record.models.push(entry);
        record.models.len() - 1
    }

    fn fell_back(&self, index: usize, fallback: Fallback) {
        let entry = &mut self.lock().models[index];
        entry.backend = Backend::Cpu;
        entry.fallback = Some(fallback);
    }
}

type MakeDetector = Box<dyn Fn(Backend) -> Result<Box<dyn Detector>, DetectError> + Send>;

/// `model` from `bytes`, which its detector checks against the pinned hash, on `backend` with the
/// CPU behind it. `backends` records where it runs. The bytes stay in memory for a CPU session
/// made later, so the file is read once.
pub fn load(
    runtime: &OrtRuntime,
    model: ModelId,
    bytes: Vec<u8>,
    backend: Backend,
    backends: &Backends,
) -> Result<Box<dyn Detector>, DetectError> {
    let runtime = runtime.clone();
    let make: MakeDetector = Box::new(move |backend| {
        Ok(match model {
            ModelId::Yolo5FaceN => Box::new(Yolo5Face::new(&runtime, &bytes, backend)?),
            ModelId::YuNet => Box::new(YuNet::new(&runtime, &bytes, backend)?),
            ModelId::PlateYolov9T => Box::new(Plate::new(&runtime, &bytes, backend)?),
        })
    });
    with_fallback(model, backend, make, backends)
}

fn with_fallback(
    model: ModelId,
    backend: Backend,
    make: MakeDetector,
    backends: &Backends,
) -> Result<Box<dyn Detector>, DetectError> {
    let counter = backends.clone();
    let make: MakeDetector = Box::new(move |backend| {
        let detector = make(backend)?;
        counter.lock().sessions += 1;
        Ok(detector)
    });
    let (detector, backend, fallback) = match make(backend) {
        Ok(detector) => (detector, backend, None),
        Err(DetectError::Failed(reason)) if backend != Backend::Cpu => {
            let fallback = Fallback {
                from: backend,
                reason,
                at_start: true,
            };
            (make(Backend::Cpu)?, Backend::Cpu, Some(fallback))
        }
        Err(e) => return Err(e),
    };
    let index = backends.add(ModelBackend {
        model,
        backend,
        fallback,
    });
    Ok(Box::new(WithCpuFallback {
        detector,
        backend,
        make,
        backends: backends.clone(),
        index,
    }))
}

struct WithCpuFallback {
    detector: Box<dyn Detector>,
    backend: Backend,
    make: MakeDetector,
    backends: Backends,
    index: usize,
}

impl Detector for WithCpuFallback {
    fn detect(&mut self, image: &Rgb8Image, thresholds: &Thresholds) -> Result<Found, DetectError> {
        let found = self.detector.detect(image, thresholds);
        self.on_cpu_if_failed(found, image, thresholds)
    }

    /// The model's input is the same on every backend, so the CPU session made later takes
    /// what this preparer makes.
    fn preparer(&self) -> Option<Preparer> {
        self.detector.preparer()
    }

    fn detect_prepared(
        &mut self,
        image: &Rgb8Image,
        input: Prepared,
        thresholds: &Thresholds,
    ) -> Result<Found, DetectError> {
        let found = self.detector.detect_prepared(image, input, thresholds);
        self.on_cpu_if_failed(found, image, thresholds)
    }
}

impl WithCpuFallback {
    fn on_cpu_if_failed(
        &mut self,
        found: Result<Found, DetectError>,
        image: &Rgb8Image,
        thresholds: &Thresholds,
    ) -> Result<Found, DetectError> {
        match found {
            Err(DetectError::Failed(reason)) if self.backend != Backend::Cpu => {
                self.detector = (self.make)(Backend::Cpu)?;
                let from = std::mem::replace(&mut self.backend, Backend::Cpu);
                let fallback = Fallback {
                    from,
                    reason,
                    at_start: false,
                };
                self.backends.fell_back(self.index, fallback);
                let mut found = self.detector.detect(image, thresholds)?;
                found.reran_on_cpu = true;
                Ok(found)
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::{DetectorHandle, prepared};
    use cloakframe_core::detection::{Detection, KindThresholds};
    use cloakframe_core::geometry::Rect;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn image(width: u32) -> Rgb8Image {
        Rgb8Image {
            width,
            height: 1,
            data: vec![0; width as usize * 3],
        }
    }

    fn thresholds() -> Thresholds {
        let t = KindThresholds::new(0.5, 0.4).unwrap();
        Thresholds { face: t, plate: t }
    }

    /// One detection as wide as the image, tagged with the backend in its score; an image of
    /// width 13 fails on the accelerator. Its prepared input is the image's width, as the real
    /// detectors prepare theirs.
    struct Scripted(Backend);

    impl Detector for Scripted {
        fn detect(&mut self, image: &Rgb8Image, _: &Thresholds) -> Result<Found, DetectError> {
            if image.width == 13 && self.0 != Backend::Cpu {
                return Err(DetectError::Failed("device removed".into()));
            }
            if image.width == 0 {
                return Err(DetectError::UnexpectedModel("bad output".into()));
            }
            let score = if self.0 == Backend::Cpu { 0.6 } else { 0.9 };
            let bbox = Rect::new(0.0, 0.0, image.width as f32, 1.0).unwrap();
            let found = Detection::new(ModelId::YuNet, score, bbox, None, None).unwrap();
            Ok(Found {
                detections: vec![found],
                ..Found::default()
            })
        }

        fn preparer(&self) -> Option<Preparer> {
            Some(Box::new(|image| Box::new(image.width)))
        }

        fn detect_prepared(
            &mut self,
            image: &Rgb8Image,
            input: Prepared,
            thresholds: &Thresholds,
        ) -> Result<Found, DetectError> {
            assert_eq!(prepared::<u32>(input), image.width);
            self.detect(image, thresholds)
        }
    }

    /// Counts the sessions made; `fail` makes making one on it fail as ORT would.
    fn maker(made: &Arc<AtomicUsize>, fail: Option<Backend>) -> MakeDetector {
        let made = Arc::clone(made);
        Box::new(move |backend| {
            made.fetch_add(1, Ordering::SeqCst);
            if Some(backend) == fail {
                return Err(DetectError::Failed("no device".into()));
            }
            Ok(Box::new(Scripted(backend)))
        })
    }

    fn score(found: &Found) -> f32 {
        found.detections[0].score()
    }

    #[test]
    fn a_failed_image_is_analyzed_again_on_the_cpu_which_keeps_the_model() {
        let made = Arc::new(AtomicUsize::new(0));
        let backends = Backends::default();
        let mut d = with_fallback(
            ModelId::YuNet,
            Backend::CoreMl,
            maker(&made, None),
            &backends,
        )
        .unwrap();
        let first = d.detect(&image(4), &thresholds()).unwrap();
        assert_eq!((score(&first), first.reran_on_cpu), (0.9, false));
        assert_eq!(backends.get()[0].backend, Backend::CoreMl);
        for _ in 0..3 {
            d.detect(&image(4), &thresholds()).unwrap();
        }
        assert_eq!(backends.sessions_made(), 1);

        let failed = d.detect(&image(13), &thresholds()).unwrap();
        assert_eq!((score(&failed), failed.reran_on_cpu), (0.6, true));
        assert_eq!(failed.detections[0].bbox().width(), 13.0);
        assert_eq!(
            backends.get(),
            [ModelBackend {
                model: ModelId::YuNet,
                backend: Backend::Cpu,
                fallback: Some(Fallback {
                    from: Backend::CoreMl,
                    reason: "device removed".into(),
                    at_start: false,
                }),
            }]
        );

        let later = d.detect(&image(4), &thresholds()).unwrap();
        assert_eq!((score(&later), later.reran_on_cpu), (0.6, false));
        assert_eq!(made.load(Ordering::SeqCst), 2);
        assert_eq!(backends.sessions_made(), 2);
    }

    /// The inference thread hands every detector the input its preparer made.
    #[test]
    fn a_prepared_image_that_fails_is_analyzed_again_on_the_cpu() {
        let made = Arc::new(AtomicUsize::new(0));
        let backends = Backends::default();
        let d = with_fallback(
            ModelId::YuNet,
            Backend::CoreMl,
            maker(&made, None),
            &backends,
        )
        .unwrap();
        let handle = DetectorHandle::spawn(d).unwrap();
        let first = handle.detect(image(4), thresholds()).unwrap();
        assert_eq!((score(&first), first.reran_on_cpu), (0.9, false));
        let failed = handle.detect(image(13), thresholds()).unwrap();
        assert_eq!((score(&failed), failed.reran_on_cpu), (0.6, true));
        let later = handle.detect(image(4), thresholds()).unwrap();
        assert_eq!((score(&later), later.reran_on_cpu), (0.6, false));
        assert_eq!(backends.get()[0].backend, Backend::Cpu);
    }

    #[test]
    fn a_model_that_cannot_start_on_the_accelerator_starts_on_the_cpu() {
        let made = Arc::new(AtomicUsize::new(0));
        let backends = Backends::default();
        let mut d = with_fallback(
            ModelId::Yolo5FaceN,
            Backend::DirectMl,
            maker(&made, Some(Backend::DirectMl)),
            &backends,
        )
        .unwrap();
        assert_eq!(score(&d.detect(&image(13), &thresholds()).unwrap()), 0.6);
        let entry = &backends.get()[0];
        assert_eq!(entry.backend, Backend::Cpu);
        let fallback = entry.fallback.as_ref().unwrap();
        assert_eq!(
            (fallback.from, fallback.at_start),
            (Backend::DirectMl, true)
        );
        assert_eq!(made.load(Ordering::SeqCst), 2);
        assert_eq!(backends.sessions_made(), 1);
    }

    #[test]
    fn model_errors_and_cpu_failures_are_not_retried() {
        let made = Arc::new(AtomicUsize::new(0));
        let backends = Backends::default();
        let mut on_gpu = with_fallback(
            ModelId::YuNet,
            Backend::CoreMl,
            maker(&made, None),
            &backends,
        )
        .unwrap();
        assert_eq!(
            on_gpu.detect(&image(0), &thresholds()),
            Err(DetectError::UnexpectedModel("bad output".into()))
        );
        assert_eq!(backends.get()[0].backend, Backend::CoreMl);

        // No CPU session for the image that failed: the error stands and the model stays put.
        let mut no_cpu = with_fallback(
            ModelId::YuNet,
            Backend::CoreMl,
            maker(&made, Some(Backend::Cpu)),
            &backends,
        )
        .unwrap();
        assert!(no_cpu.detect(&image(13), &thresholds()).is_err());
        assert_eq!(backends.get()[1].fallback, None);

        let cpu_only = with_fallback(
            ModelId::YuNet,
            Backend::Cpu,
            maker(&made, Some(Backend::Cpu)),
            &backends,
        );
        assert!(cpu_only.is_err());
        assert_eq!(backends.get().len(), 2);
    }

    #[test]
    fn each_accelerator_runs_the_models_where_plan_3_says() {
        use Backend::*;
        let models = [ModelId::Yolo5FaceN, ModelId::YuNet, ModelId::PlateYolov9T];
        for (accelerator, expected) in [
            (None, [Cpu, Cpu, Cpu]),
            (Some(CoreMl), [CoreMl, CoreMl, Cpu]),
            (Some(DirectMl), [DirectMl; 3]),
            (Some(Cuda), [Cuda; 3]),
        ] {
            for (model, backend) in models.into_iter().zip(expected) {
                assert_eq!(
                    backend_for(model, Acceleration::Gpu, accelerator),
                    backend,
                    "{model:?} {accelerator:?}"
                );
                assert_eq!(backend_for(model, Acceleration::CpuOnly, accelerator), Cpu);
            }
        }
    }
}
