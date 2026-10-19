//! The models a run detects with, and their sessions as one detector for the inference thread.

use std::fmt;
use std::path::PathBuf;

use cloakframe_core::detection::{ModelId, Thresholds};
use cloakframe_core::review::Digest;
use sha2::{Digest as _, Sha256};

use super::fallback::{self, Acceleration, Backends};
use super::runtime::OrtRuntime;
use super::{DetectError, Detector, Found, Rgb8Image};
use crate::model_files::{ModelStore, model_file};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceModel {
    /// Finds more small faces. Its upstream terms allow non-commercial use only.
    Yolo5FaceN,
    /// Faster, MIT-licensed.
    YuNet,
}

impl FaceModel {
    pub fn id(self) -> ModelId {
        match self {
            Self::Yolo5FaceN => ModelId::Yolo5FaceN,
            Self::YuNet => ModelId::YuNet,
        }
    }
}

const DEFAULT_FACE: FaceModel = FaceModel::Yolo5FaceN;

/// A run that detects nothing would publish its photos unmasked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NothingToDetect;

impl fmt::Display for NothingToDetect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("neither faces nor license plates are detected")
    }
}

impl std::error::Error for NothingToDetect {}

#[derive(Debug)]
pub enum LoadError {
    /// The model folder has no file under the model's name; it has to be downloaded.
    Missing {
        model: ModelId,
        path: PathBuf,
    },
    Read {
        path: PathBuf,
        error: std::io::Error,
    },
    Model {
        path: PathBuf,
        error: DetectError,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { path, .. } => write!(f, "the model {} is missing", path.display()),
            Self::Read { path, error } => {
                write!(f, "cannot read the model {}: {error}", path.display())
            }
            Self::Model { path, error } => {
                write!(f, "cannot load the model {}: {error}", path.display())
            }
        }
    }
}

impl std::error::Error for LoadError {}

/// A run's models as one detector, and where each of them runs.
pub struct Loaded {
    pub detector: Box<dyn Detector>,
    pub backends: Backends,
}

/// The face model, if faces are detected, and whether license plates are. Never neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectionModels {
    face: Option<FaceModel>,
    plates: bool,
}

impl Default for DetectionModels {
    /// YOLO5Face-n without plates, as 1.12.1 starts.
    fn default() -> Self {
        Self {
            face: Some(DEFAULT_FACE),
            plates: false,
        }
    }
}

impl DetectionModels {
    pub fn new(face: Option<FaceModel>, plates: bool) -> Result<Self, NothingToDetect> {
        if face.is_none() && !plates {
            return Err(NothingToDetect);
        }
        Ok(Self { face, plates })
    }

    pub fn face(&self) -> Option<FaceModel> {
        self.face
    }

    pub fn plates(&self) -> bool {
        self.plates
    }

    /// Faces first, so a photo's detections keep that order.
    pub fn ids(&self) -> impl Iterator<Item = ModelId> {
        let plate = self.plates.then_some(ModelId::PlateYolov9T);
        self.face.map(FaceModel::id).into_iter().chain(plate)
    }

    /// The models' pinned files, for the review key: analysis with other models invalidates a
    /// stored review (plan 4.2).
    pub fn digest(&self) -> Digest {
        let mut h = Sha256::new();
        for id in self.ids() {
            h.update(format!("{:?} {}\n", id.kind(), model_file(id).sha256));
        }
        Digest(h.finalize().into())
    }

    /// The models' default thresholds. The face pair is the default face model's when faces are
    /// off; it is not used then.
    pub fn default_thresholds(&self) -> Thresholds {
        Thresholds {
            face: self.face.unwrap_or(DEFAULT_FACE).id().default_thresholds(),
            plate: ModelId::PlateYolov9T.default_thresholds(),
        }
    }

    /// Sessions for the models from their files in `store`, each on the backend
    /// `fallback::backend_for` names with the CPU behind it: one session per model, input size and
    /// backend for the whole run (D7), since every model has a fixed input size. Each detector
    /// checks the bytes against the pinned hash before making a session from them (D5).
    pub fn load(
        &self,
        runtime: &OrtRuntime,
        store: &ModelStore,
        acceleration: Acceleration,
    ) -> Result<Loaded, LoadError> {
        let backends = Backends::default();
        let mut detectors = Vec::new();
        for id in self.ids() {
            let path = store.path(id);
            let bytes = match store.read(id) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(LoadError::Missing { model: id, path });
                }
                Err(error) => return Err(LoadError::Read { path, error }),
            };
            let backend = fallback::backend_for(id, acceleration);
            let detector = fallback::load(runtime, id, bytes, backend, &backends)
                .map_err(|error| LoadError::Model { path, error })?;
            detectors.push(detector);
        }
        Ok(Loaded {
            detector: Box::new(Combined(detectors)),
            backends,
        })
    }
}

/// A run's models on the one inference thread. Each finds its own kind with its own NMS; their
/// results are joined without NMS between them, so a face and a plate never suppress each other
/// (P-2).
struct Combined(Vec<Box<dyn Detector>>);

impl Detector for Combined {
    fn detect(&mut self, image: &Rgb8Image, thresholds: &Thresholds) -> Result<Found, DetectError> {
        let mut all = Found::default();
        for detector in &mut self.0 {
            let found = detector.detect(image, thresholds)?;
            all.detections.extend(found.detections);
            all.over_limit.extend(found.over_limit);
            all.reran_on_cpu |= found.reran_on_cpu;
        }
        Ok(all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloakframe_core::detection::{Detection, KindThresholds};
    use cloakframe_core::geometry::Rect;

    #[test]
    fn something_is_always_detected() {
        assert_eq!(DetectionModels::new(None, false), Err(NothingToDetect));
        let plates = DetectionModels::new(None, true).unwrap();
        assert_eq!(plates.ids().collect::<Vec<_>>(), [ModelId::PlateYolov9T]);
        let both = DetectionModels::new(Some(FaceModel::YuNet), true).unwrap();
        assert_eq!(
            both.ids().collect::<Vec<_>>(),
            [ModelId::YuNet, ModelId::PlateYolov9T]
        );
        let default = DetectionModels::default();
        assert_eq!(
            (default.face(), default.plates()),
            (Some(FaceModel::Yolo5FaceN), false)
        );
    }

    #[test]
    fn each_model_set_has_its_own_digest() {
        let sets = [
            (Some(FaceModel::Yolo5FaceN), false),
            (Some(FaceModel::YuNet), false),
            (Some(FaceModel::Yolo5FaceN), true),
            (Some(FaceModel::YuNet), true),
            (None, true),
        ];
        let digests: Vec<Digest> = sets
            .iter()
            .map(|&(face, plates)| DetectionModels::new(face, plates).unwrap().digest())
            .collect();
        for (i, d) in digests.iter().enumerate() {
            assert_eq!(
                digests.iter().filter(|e| *e == d).count(),
                1,
                "{:?}",
                sets[i]
            );
        }
        assert_eq!(DetectionModels::default().digest(), digests[0]);
    }

    #[test]
    fn default_thresholds_follow_the_models() {
        let t = DetectionModels::new(Some(FaceModel::YuNet), true)
            .unwrap()
            .default_thresholds();
        assert_eq!(t.face, ModelId::YuNet.default_thresholds());
        assert_eq!(t.plate, ModelId::PlateYolov9T.default_thresholds());
        let plates_only = DetectionModels::new(None, true).unwrap();
        assert_eq!(plates_only.default_thresholds().plate, t.plate);
    }

    struct Finds(Vec<Detection>, Vec<(ModelId, u32)>);

    impl Detector for Finds {
        fn detect(&mut self, _: &Rgb8Image, _: &Thresholds) -> Result<Found, DetectError> {
            Ok(Found {
                detections: self.0.clone(),
                over_limit: self.1.clone(),
                ..Found::default()
            })
        }
    }

    struct Fails;

    impl Detector for Fails {
        fn detect(&mut self, _: &Rgb8Image, _: &Thresholds) -> Result<Found, DetectError> {
            Err(DetectError::Failed("broken".into()))
        }
    }

    fn found(model: ModelId, score: f32) -> Detection {
        let bbox = Rect::new(0.0, 0.0, 10.0, 10.0).unwrap();
        Detection::new(model, score, bbox, None, None).unwrap()
    }

    #[test]
    fn combined_results_keep_every_kind_and_each_models_limit() {
        // A face and a plate on the same box, as P-2 case 1 asks, and a capped face model.
        let face = found(ModelId::YuNet, 0.6);
        let plate = found(ModelId::PlateYolov9T, 0.9);
        let mut combined = Combined(vec![
            Box::new(Finds(vec![face.clone()], vec![(ModelId::YuNet, 2)])),
            Box::new(Finds(vec![plate.clone()], Vec::new())),
        ]);
        let image = Rgb8Image {
            width: 10,
            height: 10,
            data: vec![0; 300],
        };
        let t = KindThresholds::new(0.5, 0.4).unwrap();
        let thresholds = Thresholds { face: t, plate: t };
        let all = combined.detect(&image, &thresholds).unwrap();
        assert_eq!(all.detections, [face, plate]);
        assert_eq!(all.over_limit, [(ModelId::YuNet, 2)]);

        let mut failing = Combined(vec![
            Box::new(Finds(Vec::new(), Vec::new())),
            Box::new(Fails),
        ]);
        assert_eq!(
            failing.detect(&image, &thresholds),
            Err(DetectError::Failed("broken".into()))
        );
    }
}
