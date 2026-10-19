//! The detectors on the pinned ONNX Runtime against the 1.12.1 detection fixtures
//! (tests/fixtures/detect/<model>, 6 images x 3 score thresholds). The face fixtures used NMS
//! 0.4; 1.12.1 ignored that setting for plates, whose graph suppressed at 0.45.
//!
//! YOLO5Face-n: 1.12.1 kept only the 2,000 best candidates before NMS. The crowd fixture exceeds
//! that at every threshold (defects/D11/observed_1121.json), so there every 1.12.1 detection must
//! still be found and the result must be all 280 faces (defects/D11/expected.json); elsewhere the
//! results match. YuNet and the plate model match 1.12.1 everywhere.
//!
//! Needs, as CI provides them (plan 9.1: missing assets fail the test rather than skip it):
//! - `CLOAKFRAME_ORT_LIBRARY`: the library `tools/fetch_native.py` prints for this platform
//! - `CLOAKFRAME_TEST_ASSETS`: the folder `tools/fetch_test_assets.py` filled
//! - `ORT_DISABLE_TELEMETRY=1`

mod common;

use std::path::Path;
use std::sync::OnceLock;

use cloakframe_core::detection::{DetectionKind, KindThresholds, ModelId, Thresholds};
use cloakframe_core::geometry::Rect;
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::plate::Plate;
use cloakframe_engine::detect::runtime::OrtRuntime;
use cloakframe_engine::detect::yolo5face::Yolo5Face;
use cloakframe_engine::detect::yunet::YuNet;
use cloakframe_engine::detect::{Detector, Rgb8Image};
use cloakframe_engine::photo::budget::MemoryBudget;
use cloakframe_engine::photo::{color, decode};
use common::{fixtures, required_env};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const MIN_IOU: f32 = 0.9999;
const MAX_SCORE_DIFF: f32 = 1e-5;
const MAX_ROLL_DIFF: f32 = 1e-4;
const CROWD: &str = "crowd-280-faces";
const CROWD_FACES: usize = 280;

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

/// ORT has one environment per process, shared by the tests here.
fn runtime() -> &'static OrtRuntime {
    static RUNTIME: OnceLock<OrtRuntime> = OnceLock::new();
    RUNTIME.get_or_init(|| OrtRuntime::load(&required_env("CLOAKFRAME_ORT_LIBRARY")).unwrap())
}

fn model_bytes(assets: &Path, name: &str) -> Vec<u8> {
    let spec: Assets =
        serde_json::from_str(&std::fs::read_to_string(fixtures().join("assets.json")).unwrap())
            .unwrap();
    let model = spec.models.iter().find(|m| m.name == name).unwrap();
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
    let assets = required_env("CLOAKFRAME_TEST_ASSETS");
    let mut detector =
        Yolo5Face::new(runtime(), &model_bytes(&assets, "yolov5n_face.onnx")).unwrap();
    compare(&mut detector, ModelId::Yolo5FaceN, |stem, found_1121| {
        if stem == CROWD {
            CROWD_FACES
        } else {
            found_1121
        }
    });
}

#[test]
fn yunet_matches_the_1_12_1_fixtures() {
    let assets = required_env("CLOAKFRAME_TEST_ASSETS");
    let model = model_bytes(&assets, "face_detection_yunet_2023mar.onnx");
    let mut detector = YuNet::new(runtime(), &model).unwrap();
    compare(&mut detector, ModelId::YuNet, |_, found_1121| found_1121);
}

#[test]
fn plate_matches_the_1_12_1_fixtures() {
    let assets = required_env("CLOAKFRAME_TEST_ASSETS");
    let model = model_bytes(&assets, "yolo-v9-t-512-license-plates-end2end.onnx");
    let mut detector = Plate::new(runtime(), &model).unwrap();
    compare(&mut detector, ModelId::PlateYolov9T, |_, found_1121| {
        found_1121
    });
}

/// The plate graph returns at most 100 boxes and suppresses at IoU 0.45. With NMS off here, the
/// car photo tiled 3x3 gives more than 100 boxes: the graph's limits no longer apply (with them,
/// 35 come out). With NMS at the graph's 0.45, each tile's plate is found once.
#[test]
fn the_plate_graph_limits_are_lifted() {
    let assets = required_env("CLOAKFRAME_TEST_ASSETS");
    let model = model_bytes(&assets, "yolo-v9-t-512-license-plates-end2end.onnx");
    let mut detector = Plate::new(runtime(), &model).unwrap();
    let car = detection_copy(&assets.join("car_image.webp"));
    let (w, h) = (car.width as usize, car.height as usize);
    let mut tiled = Rgb8Image {
        width: car.width * 3,
        height: car.height * 3,
        data: Vec::with_capacity(9 * car.data.len()),
    };
    for _ in 0..3 {
        for row in car.data.chunks_exact(w * 3) {
            for _ in 0..3 {
                tiled.data.extend_from_slice(row);
            }
        }
    }
    assert_eq!(tiled.data.len(), 9 * w * h * 3);
    let thresholds = |plate: KindThresholds| Thresholds { face: plate, plate };
    let unsuppressed = thresholds(KindThresholds::new(0.001, 1.0).unwrap());
    let found = detector.detect(&tiled, &unsuppressed).unwrap();
    assert!(found.detections.len() > 100, "{}", found.detections.len());
    assert_eq!(found.over_limit, 0);

    let plates = thresholds(KindThresholds::new(0.25, 0.45).unwrap());
    let found = detector.detect(&tiled, &plates).unwrap();
    let mut tiles: Vec<(usize, usize)> = found
        .detections
        .iter()
        .map(|d| {
            let b = d.bbox();
            let centre = (b.x() + b.width() / 2.0, b.y() + b.height() / 2.0);
            (centre.0 as usize / w, centre.1 as usize / h)
        })
        .collect();
    tiles.sort_unstable();
    tiles.dedup();
    assert_eq!((found.detections.len(), tiles.len()), (9, 9));
}

/// Runs `detector` on the fixture images at each threshold and compares with
/// `tests/fixtures/detect/<model>`. `expected` gives the count to find from the image stem and
/// the count 1.12.1 found.
fn compare(detector: &mut dyn Detector, id: ModelId, expected: impl Fn(&str, usize) -> usize) {
    let model = match id {
        ModelId::Yolo5FaceN => "yolo5face",
        ModelId::YuNet => "yunet",
        ModelId::PlateYolov9T => "plate",
    };
    let assets = required_env("CLOAKFRAME_TEST_ASSETS");
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
            let file = fixtures().join(format!("detect/{model}/{stem}@{score:.2}.json"));
            let want: Expected =
                serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
            assert_eq!(want.decoded_size, [image.width, image.height], "{case}");
            let thresholds = match id.kind() {
                DetectionKind::Face => KindThresholds::new(score, 0.4),
                DetectionKind::Plate => KindThresholds::new(score, 0.45),
            }
            .unwrap();
            let thresholds = Thresholds {
                face: thresholds,
                plate: thresholds,
            };
            let found = detector.detect(&image, &thresholds).unwrap();
            cases += 1;
            if found.over_limit != 0 {
                failures.push(format!("{case}: {} over the limit", found.over_limit));
            }
            let expected = expected(stem, want.detections.len());
            if found.detections.len() != expected {
                failures.push(format!(
                    "{case}: {} detections, expected {expected} (1.12.1 found {})",
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
        "{model}: {cases} cases, min IoU {min_iou}, max score diff {max_score:e}, max roll diff {max_roll:e}"
    );
    assert_eq!(cases, 18);
    assert!(failures.is_empty(), "{failures:#?}");
}
