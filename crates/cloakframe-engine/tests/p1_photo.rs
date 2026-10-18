//! P-1 for photos on the real detector (tests/fixtures/defects/P-1, second and third cases): with
//! the per-image limit lowered to 100, the 280-face crowd photo is held for review with capped
//! candidates and nothing is written; only an explicit export with warnings publishes it, and that
//! is not a normal completion. At the regular limit the same photo has no problem (D11).
//!
//! Needs `CLOAKFRAME_ORT_LIBRARY`, `CLOAKFRAME_TEST_ASSETS` and `ORT_DISABLE_TELEMETRY=1`, as
//! detect_fixtures.rs does.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use cloakframe_core::coverage::{CoverageEvidence, Problem};
use cloakframe_core::detection::ModelId;
use cloakframe_core::item::{ItemId, ItemState, PublishBasis, ReviewMode};
use cloakframe_core::review::PhotoEdits;
use cloakframe_engine::detect::DetectorHandle;
use cloakframe_engine::detect::runtime::OrtRuntime;
use cloakframe_engine::detect::yolo5face::Yolo5Face;
use cloakframe_engine::photo::process::{PhotoProcessor, PhotoSettings};
use cloakframe_engine::run::{Decision, Job, JobItem, RunReport, Start, start};

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

fn detector(limit: Option<usize>) -> DetectorHandle {
    let model =
        std::fs::read(required_env("CLOAKFRAME_TEST_ASSETS").join("yolov5n_face.onnx")).unwrap();
    let mut detector = Yolo5Face::new(runtime(), &model).unwrap();
    if let Some(limit) = limit {
        detector = detector.with_detection_limit(limit);
    }
    DetectorHandle::spawn(Box::new(detector)).unwrap()
}

fn run(item: JobItem, detector: DetectorHandle) -> RunReport {
    let job = Job {
        items: vec![item],
        review: ReviewMode::Off,
        workers: 1,
    };
    let processor = PhotoProcessor::new(PhotoSettings::default()).unwrap();
    start(job, Arc::new(processor), detector).unwrap().wait()
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
fn a_capped_crowd_photo_is_published_only_when_exported_with_warnings() {
    let out = out_dir("capped");
    let destination = out.0.join("crowd.png");
    let item = |start| JobItem {
        id: ItemId(0),
        input: crowd(),
        destination: destination.clone(),
        start,
    };
    let report = run(
        item(Start::Fresh {
            needs_review: false,
        }),
        detector(Some(100)),
    );
    let outcome = &report.items[0];
    assert_eq!(
        outcome.state,
        ItemState::NeedsReview,
        "{:?}",
        outcome.failure
    );
    assert_eq!(
        outcome.analysis.evidence,
        [CoverageEvidence::unresolved(Problem::CappedCandidates {
            model: ModelId::Yolo5FaceN,
            frame: None,
            kept: 100,
            found: CROWD_FACES as u32,
        })]
    );
    assert!(!out.0.exists(), "nothing is written before review");

    let analysis = outcome.analysis.clone();
    let report = run(
        item(Start::Reviewed {
            decision: Decision::ExportWithWarnings,
            analysis: analysis.clone(),
            edits: PhotoEdits::default(),
        }),
        detector(Some(100)),
    );
    assert_eq!(
        report.items[0].state,
        ItemState::Published(PublishBasis::ExportedWithWarnings),
        "{:?}",
        report.items[0].failure
    );
    assert_eq!(report.completed(), 0);
    let masked = image::open(&destination).unwrap().to_rgb8();
    for d in &analysis.detections {
        let b = d.bbox();
        let centre = masked.get_pixel(
            (b.x() + b.width() / 2.0) as u32,
            (b.y() + b.height() / 2.0) as u32,
        );
        assert_eq!(centre.0, [0, 0, 0], "{b:?}");
    }
}

#[test]
fn at_the_regular_limit_the_crowd_photo_has_no_problem() {
    let out = out_dir("regular");
    let report = run(
        JobItem {
            id: ItemId(0),
            input: crowd(),
            destination: out.0.join("crowd.png"),
            start: Start::Fresh {
                needs_review: false,
            },
        },
        detector(None),
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
