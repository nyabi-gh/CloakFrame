//! The models a run detects with, and their sessions as one detector for the inference thread.

use std::fmt;
use std::path::{Path, PathBuf};

use cloakframe_core::detection::{ModelId, Thresholds};
use cloakframe_core::review::Digest;
use sha2::{Digest as _, Sha256};

use super::plate::{self, Plate};
use super::runtime::OrtRuntime;
use super::yolo5face::{self, Yolo5Face};
use super::yunet::{self, YuNet};
use super::{DetectError, Detector, Found, Rgb8Image};

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

/// The published file name and pinned SHA-256 of a model (`manifests/native.toml`).
fn pinned(model: ModelId) -> (&'static str, &'static str) {
    match model {
        ModelId::Yolo5FaceN => (yolo5face::FILE_NAME, yolo5face::MODEL_SHA256),
        ModelId::YuNet => (yunet::FILE_NAME, yunet::MODEL_SHA256),
        ModelId::PlateYolov9T => (plate::FILE_NAME, plate::MODEL_SHA256),
    }
}

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
            h.update(format!("{:?} {}\n", id.kind(), pinned(id).1));
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

    /// Sessions for the models from their files in `dir`, under the published names. Each
    /// detector checks the bytes against the pinned hash before making a session from them.
    pub fn load(&self, runtime: &OrtRuntime, dir: &Path) -> Result<Box<dyn Detector>, LoadError> {
        let mut detectors: Vec<Box<dyn Detector>> = Vec::new();
        for id in self.ids() {
            let path = dir.join(pinned(id).0);
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) => return Err(LoadError::Read { path, error }),
            };
            let detector: Result<Box<dyn Detector>, DetectError> = match id {
                ModelId::Yolo5FaceN => {
                    Yolo5Face::new(runtime, &bytes).map(|d| Box::new(d) as Box<dyn Detector>)
                }
                ModelId::YuNet => YuNet::new(runtime, &bytes).map(|d| Box::new(d) as _),
                ModelId::PlateYolov9T => Plate::new(runtime, &bytes).map(|d| Box::new(d) as _),
            };
            detectors.push(detector.map_err(|error| LoadError::Model { path, error })?);
        }
        Ok(Box::new(Combined(detectors)))
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

    #[test]
    fn the_file_names_are_the_published_ones() {
        let manifest = include_str!("../../../../manifests/native.toml");
        for id in [ModelId::Yolo5FaceN, ModelId::YuNet, ModelId::PlateYolov9T] {
            let (name, sha256) = pinned(id);
            let entry = manifest
                .split("[[artifact]]")
                .find(|e| e.contains(&format!("sha256 = \"{sha256}\"")))
                .unwrap_or_else(|| panic!("{id:?} is not in the manifest"));
            assert!(entry.contains(&format!("/{name}\"\n")), "{id:?}: {entry}");
        }
    }

    struct Finds(Vec<Detection>, Vec<(ModelId, u32)>);

    impl Detector for Finds {
        fn detect(&mut self, _: &Rgb8Image, _: &Thresholds) -> Result<Found, DetectError> {
            Ok(Found {
                detections: self.0.clone(),
                over_limit: self.1.clone(),
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
