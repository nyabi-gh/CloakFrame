//! D7: a run loads only the models it detects with, and each model, input size and backend gets
//! one session for the whole run, counted by the sessions the run makes. Every model has a fixed
//! input size, so that is one session per model and backend. Videos join the same run in M5. That
//! nothing is loaded before the scan finds media is checked through the CLI (cli_models.rs).
//!
//! Needs `CLOAKFRAME_ORT_LIBRARY`, `CLOAKFRAME_TEST_ASSETS` and `ORT_DISABLE_TELEMETRY=1`, as
//! detect_fixtures.rs does.

mod common;

use std::sync::{Arc, OnceLock};

use cloakframe_core::detection::ModelId;
use cloakframe_core::item::{ItemId, ItemState, PublishBasis, ReviewMode};
use cloakframe_engine::detect::DetectorHandle;
use cloakframe_engine::detect::fallback::Acceleration;
use cloakframe_engine::detect::models::{DetectionModels, FaceModel, LoadError};
use cloakframe_engine::detect::runtime::OrtRuntime;
use cloakframe_engine::model_files::{ModelStore, model_file};
use cloakframe_engine::photo::process::{PhotoProcessor, PhotoSettings};
use cloakframe_engine::run::{Job, JobItem, Start, start};
use common::{TestDir, required_env};

const PHOTOS: [&str; 4] = ["astronaut.png", "lena.jpg", "messi5.jpg", "car_image.webp"];

fn runtime() -> &'static OrtRuntime {
    static RUNTIME: OnceLock<OrtRuntime> = OnceLock::new();
    RUNTIME.get_or_init(|| OrtRuntime::load(&required_env("CLOAKFRAME_ORT_LIBRARY")).unwrap())
}

fn assets() -> ModelStore {
    ModelStore::new(required_env("CLOAKFRAME_TEST_ASSETS"))
}

/// Runs the photos with `models` on two workers and returns the sessions the run made.
fn sessions_for(models: DetectionModels, acceleration: Acceleration) -> usize {
    let out = TestDir::new();
    let loaded = models.load(runtime(), &assets(), acceleration).unwrap();
    let backends = loaded.backends.clone();
    let job = Job {
        items: PHOTOS
            .iter()
            .enumerate()
            .map(|(i, name)| JobItem {
                id: ItemId(i as u64),
                input: required_env("CLOAKFRAME_TEST_ASSETS").join(name),
                destination: out.path().join(name),
                start: Start::Fresh {
                    needs_review: false,
                },
            })
            .collect(),
        review: ReviewMode::Off,
        workers: 2,
    };
    let processor = PhotoProcessor::new(PhotoSettings {
        models,
        thresholds: models.default_thresholds(),
        ..PhotoSettings::default()
    })
    .unwrap();
    let handle = DetectorHandle::spawn(loaded.detector).unwrap();
    let report = start(job, Arc::new(processor), handle).unwrap().wait();
    for outcome in &report.items {
        assert_eq!(
            outcome.state,
            ItemState::Published(PublishBasis::AutomaticUnreviewed),
            "{}: {:?}",
            outcome.input.display(),
            outcome.failure
        );
    }
    assert_eq!(backends.get().len(), models.ids().count());
    backends.sessions_made()
}

#[test]
fn each_model_has_one_session_for_the_whole_run() {
    let faces = DetectionModels::default();
    let both = DetectionModels::new(Some(FaceModel::YuNet), true).unwrap();
    for acceleration in [Acceleration::Gpu, Acceleration::CpuOnly] {
        assert_eq!(sessions_for(faces, acceleration), 1, "{acceleration:?}");
        assert_eq!(sessions_for(both, acceleration), 2, "{acceleration:?}");
    }
}

#[test]
fn only_the_models_the_run_detects_with_are_read() {
    let dir = TestDir::new();
    let store = ModelStore::new(dir.path());
    let yunet = model_file(ModelId::YuNet).name;
    std::fs::copy(
        required_env("CLOAKFRAME_TEST_ASSETS").join(yunet),
        dir.path().join(yunet),
    )
    .unwrap();
    let faces = DetectionModels::new(Some(FaceModel::YuNet), false).unwrap();
    let loaded = faces
        .load(runtime(), &store, Acceleration::CpuOnly)
        .unwrap();
    assert_eq!(loaded.backends.sessions_made(), 1);

    let with_plates = DetectionModels::new(Some(FaceModel::YuNet), true).unwrap();
    match with_plates.load(runtime(), &store, Acceleration::CpuOnly) {
        Err(LoadError::Missing { model, path }) => {
            assert_eq!(model, ModelId::PlateYolov9T);
            assert_eq!(path, store.path(ModelId::PlateYolov9T));
        }
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("loaded without the plate model"),
    }
}
