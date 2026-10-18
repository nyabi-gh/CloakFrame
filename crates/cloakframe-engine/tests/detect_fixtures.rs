//! YOLO5Face-n on the pinned ONNX Runtime against the 1.12.1 detection fixtures
//! (tests/fixtures/detect/yolo5face, 6 images x 3 score thresholds, NMS 0.4).
//!
//! Needs, as CI provides them (plan 9.1: missing assets fail the test rather than skip it):
//! - `CLOAKFRAME_ORT_LIBRARY`: the library `tools/fetch_native.py` prints for this platform
//! - `CLOAKFRAME_TEST_ASSETS`: the folder `tools/fetch_test_assets.py` filled
//! - `ORT_DISABLE_TELEMETRY=1`

use std::path::{Path, PathBuf};

use cloakframe_core::detection::{KindThresholds, Thresholds};
use cloakframe_core::geometry::Rect;
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::runtime::OrtRuntime;
use cloakframe_engine::detect::yolo5face::Yolo5Face;
use cloakframe_engine::detect::{Detector, Rgb8Image};
use cloakframe_engine::photo::budget::MemoryBudget;
use cloakframe_engine::photo::{color, decode};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const MIN_IOU: f32 = 0.9999;
const MAX_SCORE_DIFF: f32 = 1e-5;
const MAX_ROLL_DIFF: f32 = 1e-4;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn required_env(name: &str, how: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{name} is not set: {how}"))
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    sha256: String,
}

#[derive(Deserialize)]
struct Assets {
    models: Vec<Asset>,
}

#[derive(Deserialize)]
struct Expected {
    decoded_size: [u32; 2],
    detections: Vec<ExpectedDetection>,
}

#[derive(Deserialize)]
struct ExpectedDetection {
    #[serde(rename = "box")]
    bbox: [f32; 4],
    score: f32,
    roll: f32,
    has_pose: bool,
}

fn model_bytes(assets: &Path) -> Vec<u8> {
    let spec: Assets =
        serde_json::from_str(&std::fs::read_to_string(fixtures().join("assets.json")).unwrap())
            .unwrap();
    let model = spec
        .models
        .iter()
        .find(|m| m.name == "yolov5n_face.onnx")
        .unwrap();
    let bytes = std::fs::read(assets.join(&model.name)).unwrap();
    let digest: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(digest, model.sha256, "{}", model.name);
    bytes
}

/// The detection copy the photo pipeline makes: decode, EXIF orientation, color policy.
fn detection_copy(path: &Path) -> Rgb8Image {
    let source = decode::read_source(path).unwrap();
    let mut decoded =
        decode::decode(&source, &MemoryBudget::new(1 << 30), &CancelToken::new()).unwrap();
    let outcome = color::apply(&source, &mut decoded.image).unwrap();
    color::detection_srgb8(&decoded.image, outcome).unwrap()
}

#[test]
fn yolo5face_matches_the_1_12_1_fixtures() {
    let library = required_env(
        "CLOAKFRAME_ORT_LIBRARY",
        "python -I tools/fetch_native.py onnxruntime <platform> <dir> prints it",
    );
    let assets = required_env(
        "CLOAKFRAME_TEST_ASSETS",
        "python -I tools/fetch_test_assets.py <dir> fills it",
    );
    let runtime = OrtRuntime::load(&library).unwrap();
    let mut detector = Yolo5Face::new(&runtime, &model_bytes(&assets)).unwrap();

    let inputs = fixtures().join("detect/inputs");
    let images = [
        assets.join("astronaut.png"),
        assets.join("messi5.jpg"),
        assets.join("lena.jpg"),
        assets.join("car_image.webp"),
        inputs.join("astronaut-scales.png"),
        inputs.join("crowd-280-faces.png"),
    ];
    let mut failures = Vec::new();
    let (mut cases, mut min_iou, mut max_score, mut max_roll) = (0, 1.0f32, 0.0f32, 0.0f32);
    for path in &images {
        let image = detection_copy(path);
        let name = path.file_name().unwrap().to_str().unwrap();
        let stem = path.file_stem().unwrap().to_str().unwrap();
        for score in [0.1f32, 0.25, 0.5] {
            let case = format!("{name}@{score:.2}");
            let file = fixtures().join(format!("detect/yolo5face/{stem}@{score:.2}.json"));
            let want: Expected =
                serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
            assert_eq!(want.decoded_size, [image.width, image.height], "{case}");
            let thresholds = Thresholds {
                face: KindThresholds::new(score, 0.4).unwrap(),
                ..Thresholds::default()
            };
            let found = detector.detect(&image, &thresholds).unwrap();
            cases += 1;
            if found.over_limit != 0 {
                failures.push(format!("{case}: {} over the limit", found.over_limit));
            }
            if found.detections.len() != want.detections.len() {
                failures.push(format!(
                    "{case}: {} detections, 1.12.1 found {}",
                    found.detections.len(),
                    want.detections.len()
                ));
            }
            for w in &want.detections {
                let rect = Rect::new(w.bbox[0], w.bbox[1], w.bbox[2], w.bbox[3]).unwrap();
                let Some(got) = found
                    .detections
                    .iter()
                    .max_by(|a, b| rect.iou(&a.bbox()).total_cmp(&rect.iou(&b.bbox())))
                else {
                    failures.push(format!("{case}: nothing for {:?}", w.bbox));
                    continue;
                };
                let iou = rect.iou(&got.bbox());
                let dscore = (got.score() - w.score).abs();
                min_iou = min_iou.min(iou);
                max_score = max_score.max(dscore);
                if iou < MIN_IOU || dscore > MAX_SCORE_DIFF {
                    failures.push(format!(
                        "{case}: {:?} IoU {iou}, score off by {dscore}",
                        w.bbox
                    ));
                }
                match (w.has_pose, got.roll()) {
                    (true, Some(roll)) => {
                        let droll = (roll - w.roll).abs();
                        max_roll = max_roll.max(droll);
                        if droll > MAX_ROLL_DIFF {
                            failures.push(format!("{case}: {:?} roll off by {droll}", w.bbox));
                        }
                    }
                    (false, None) => {}
                    (want_pose, got) => failures.push(format!(
                        "{case}: {:?} pose {want_pose} in 1.12.1, roll {got:?}",
                        w.bbox
                    )),
                }
            }
        }
    }
    println!(
        "{cases} cases, min IoU {min_iou}, max score diff {max_score:e}, max roll diff {max_roll:e}"
    );
    assert_eq!(cases, 18);
    assert!(failures.is_empty(), "{failures:#?}");
}
