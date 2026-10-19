//! D11 and P-1 for photos on the real detector: every detection NMS keeps is masked, so the photo
//! has no capped candidates however many faces it holds. The hold for unresolved problems is
//! checked with a fake detector in photo_pipeline.rs.
//!
//! Needs `CLOAKFRAME_ORT_LIBRARY`, `CLOAKFRAME_TEST_ASSETS` and `ORT_DISABLE_TELEMETRY=1`, as
//! detect_fixtures.rs does.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use cloakframe_core::detection::KindThresholds;
use cloakframe_core::item::{ItemId, ItemState, PublishBasis, ReviewMode};
use cloakframe_engine::detect::DetectorHandle;
use cloakframe_engine::detect::runtime::OrtRuntime;
use cloakframe_engine::detect::yolo5face::Yolo5Face;
use cloakframe_engine::photo::process::{PhotoProcessor, PhotoSettings};
use cloakframe_engine::run::{Job, JobItem, RunReport, Start, start};

const CROWD_FACES: usize = 280;

fn required_env(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{name} is not set (see CONTRIBUTING.md)"))
}

/// ORT has one environment per process, shared by the tests here.
fn runtime() -> &'static OrtRuntime {
    static RUNTIME: OnceLock<OrtRuntime> = OnceLock::new();
    RUNTIME.get_or_init(|| OrtRuntime::load(&required_env("CLOAKFRAME_ORT_LIBRARY")).unwrap())
}

fn detector() -> DetectorHandle {
    let model =
        std::fs::read(required_env("CLOAKFRAME_TEST_ASSETS").join("yolov5n_face.onnx")).unwrap();
    DetectorHandle::spawn(Box::new(Yolo5Face::new(runtime(), &model).unwrap())).unwrap()
}

fn run_with(item: JobItem, settings: PhotoSettings) -> RunReport {
    let job = Job {
        items: vec![item],
        review: ReviewMode::Off,
        workers: 1,
    };
    let processor = PhotoProcessor::new(settings).unwrap();
    start(job, Arc::new(processor), detector()).unwrap().wait()
}

struct Out(PathBuf);

impl Drop for Out {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn crowd() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/detect/inputs/crowd-280-faces.png")
}

fn out_dir(name: &str) -> Out {
    let dir = std::env::temp_dir().join(format!("cloakframe-p1-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Out(dir)
}

#[test]
fn every_face_left_after_nms_is_masked() {
    // A loose NMS keeps overlapping boxes, so far more than 1.12.1's 300-per-image limit survive.
    let out = out_dir("loose");
    let destination = out.0.join("crowd.png");
    let mut settings = PhotoSettings::default();
    settings.thresholds.face = KindThresholds::new(0.1, 0.9).unwrap();
    let report = run_with(
        JobItem {
            id: ItemId(0),
            input: crowd(),
            destination: destination.clone(),
            start: Start::Fresh {
                needs_review: false,
            },
        },
        settings,
    );
    let outcome = &report.items[0];
    assert_eq!(
        outcome.state,
        ItemState::Published(PublishBasis::AutomaticUnreviewed),
        "{:?} {:?}",
        outcome.failure,
        outcome.analysis.evidence
    );
    assert!(outcome.analysis.evidence.is_empty());
    let detections = &outcome.analysis.detections;
    assert!(detections.len() > 300, "{} detections", detections.len());
    let masked = image::open(&destination).unwrap().to_rgb8();
    for d in detections {
        let b = d.bbox();
        let centre = masked.get_pixel(
            (b.x() + b.width() / 2.0) as u32,
            (b.y() + b.height() / 2.0) as u32,
        );
        assert_eq!(centre.0, [0, 0, 0], "{b:?}");
    }
}

#[test]
fn the_crowd_photo_has_every_face_and_no_problem() {
    let out = out_dir("regular");
    let report = run_with(
        JobItem {
            id: ItemId(0),
            input: crowd(),
            destination: out.0.join("crowd.png"),
            start: Start::Fresh {
                needs_review: false,
            },
        },
        PhotoSettings::default(),
    );
    let outcome = &report.items[0];
    assert_eq!(
        outcome.state,
        ItemState::Published(PublishBasis::AutomaticUnreviewed),
        "{:?}",
        outcome.failure
    );
    assert_eq!(outcome.analysis.detections.len(), CROWD_FACES);
    assert!(outcome.analysis.evidence.is_empty());
}
