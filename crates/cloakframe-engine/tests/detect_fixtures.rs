//! The detectors on the pinned ONNX Runtime against the 1.12.1 detection fixtures
//! (tests/fixtures/detect/<model>, 6 images x 3 score thresholds), on the CPU and on this
//! platform's accelerator. The face fixtures used NMS 0.4; 1.12.1 ignored that setting for
//! plates, whose graph suppressed at 0.45.
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
use cloakframe_engine::detect::fallback::{self, Acceleration, Backends};
use cloakframe_engine::detect::plate::{self, Plate};
use cloakframe_engine::detect::runtime::{Backend, OrtRuntime};
use cloakframe_engine::detect::yolo5face::{self, Yolo5Face};
use cloakframe_engine::detect::yunet::{self, YuNet};
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
const MODELS: [ModelId; 3] = [ModelId::Yolo5FaceN, ModelId::YuNet, ModelId::PlateYolov9T];

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

/// The model's file from the test assets, checked against tests/fixtures/assets.json.
fn model_bytes(model: ModelId) -> Vec<u8> {
    let name = match model {
        ModelId::Yolo5FaceN => yolo5face::FILE_NAME,
        ModelId::YuNet => yunet::FILE_NAME,
        ModelId::PlateYolov9T => plate::FILE_NAME,
    };
    let spec: Assets =
        serde_json::from_str(&std::fs::read_to_string(fixtures().join("assets.json")).unwrap())
            .unwrap();
    let asset = spec.models.iter().find(|m| m.name == name).unwrap();
    let bytes = std::fs::read(required_env("CLOAKFRAME_TEST_ASSETS").join(name)).unwrap();
    let digest: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(digest, asset.sha256, "{name}");
    bytes
}

fn on_cpu(model: ModelId) -> Box<dyn Detector> {
    let (runtime, bytes) = (runtime(), model_bytes(model));
    match model {
        ModelId::Yolo5FaceN => Box::new(Yolo5Face::new(runtime, &bytes, Backend::Cpu).unwrap()),
        ModelId::YuNet => Box::new(YuNet::new(runtime, &bytes, Backend::Cpu).unwrap()),
        ModelId::PlateYolov9T => Box::new(Plate::new(runtime, &bytes, Backend::Cpu).unwrap()),
    }
}

/// The count to find: every 1.12.1 detection, and for YOLO5Face-n all 280 crowd faces.
fn expected_count(model: ModelId) -> impl Fn(&str, usize) -> usize {
    move |stem, found_1121| {
        if model == ModelId::Yolo5FaceN && stem == CROWD {
            CROWD_FACES
        } else {
            found_1121
        }
    }
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
    let id = ModelId::Yolo5FaceN;
    compare(on_cpu(id).as_mut(), id, expected_count(id));
}

#[test]
fn yunet_matches_the_1_12_1_fixtures() {
    let id = ModelId::YuNet;
    compare(on_cpu(id).as_mut(), id, expected_count(id));
}

#[test]
fn plate_matches_the_1_12_1_fixtures() {
    let id = ModelId::PlateYolov9T;
    compare(on_cpu(id).as_mut(), id, expected_count(id));
}

/// Each model where a run puts it, with the CPU behind it. On macOS the face models must stay on
/// CoreML throughout; the Windows runners have no GPU, so there DirectML may give way at start.
#[test]
fn the_models_match_the_fixtures_where_a_run_puts_them() {
    for id in MODELS {
        let backends = Backends::default();
        let backend = fallback::backend_for(id, Acceleration::Gpu);
        let mut detector =
            fallback::load(runtime(), id, model_bytes(id), backend, &backends).unwrap();
        compare(detector.as_mut(), id, expected_count(id));
        let entry = &backends.get()[0];
        println!(
            "{id:?}: asked for {backend}, ran on {} {:?}",
            entry.backend, entry.fallback
        );
        if cfg!(target_os = "macos") {
            assert_eq!((entry.backend, &entry.fallback), (backend, &None), "{id:?}");
        }
        if let Some(f) = &entry.fallback {
            assert!(f.at_start, "{id:?} {f:?}");
        }
    }
}

/// A real accelerator failure: CoreML rejects the plate graph's empty tensor when nothing is
/// found. The plate model is forced onto CoreML here; the photo with no plate is analyzed again
/// on the CPU, and the model stays there.
#[cfg(target_os = "macos")]
#[test]
fn an_image_that_fails_on_coreml_is_analyzed_again_on_the_cpu() {
    let id = ModelId::PlateYolov9T;
    let assets = required_env("CLOAKFRAME_TEST_ASSETS");
    let car = detection_copy(&assets.join("car_image.webp"));
    let empty = detection_copy(&fixtures().join("detect/inputs/astronaut-scales.png"));
    let t = id.default_thresholds();
    let thresholds = Thresholds { face: t, plate: t };
    let mut cpu = on_cpu(id);
    let backends = Backends::default();
    let mut forced =
        fallback::load(runtime(), id, model_bytes(id), Backend::CoreMl, &backends).unwrap();

    let first = forced.detect(&car, &thresholds).unwrap();
    assert!(!first.reran_on_cpu);
    assert_eq!(first.detections.len(), 1);
    assert_eq!(backends.get()[0].backend, Backend::CoreMl);

    let failed = forced.detect(&empty, &thresholds).unwrap();
    assert!(failed.reran_on_cpu);
    assert_eq!(
        failed,
        cpu.detect(&empty, &thresholds)
            .map(|mut f| {
                f.reran_on_cpu = true;
                f
            })
            .unwrap()
    );
    let entry = &backends.get()[0];
    assert_eq!(entry.backend, Backend::Cpu);
    let f = entry.fallback.as_ref().unwrap();
    assert_eq!((f.from, f.at_start), (Backend::CoreMl, false));
    assert!(f.reason.contains("zero elements"), "{}", f.reason);

    let later = forced.detect(&car, &thresholds).unwrap();
    assert!(!later.reran_on_cpu);
    assert_eq!(later, cpu.detect(&car, &thresholds).unwrap());
}

/// CoreML keeps compiled models per ORT build and model, so a new runtime or model never loads
/// an old one; a graph with replaced initializers is not cached.
#[cfg(target_os = "macos")]
#[test]
fn coreml_caches_compiled_models_per_runtime_build_and_model() {
    let dir = common::TestDir::new();
    let cached = runtime()
        .clone()
        .with_coreml_cache(dir.path().to_path_buf());
    YuNet::new(&cached, &model_bytes(ModelId::YuNet), Backend::CoreMl).unwrap();
    Plate::new(
        &cached,
        &model_bytes(ModelId::PlateYolov9T),
        Backend::CoreMl,
    )
    .unwrap();
    let builds: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(builds.len(), 1, "{builds:?}");
    assert_eq!(builds[0].file_name().unwrap().len(), 16);
    let models: Vec<_> = std::fs::read_dir(&builds[0])
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(models, [yunet::MODEL_SHA256]);
    assert!(
        std::fs::read_dir(builds[0].join(yunet::MODEL_SHA256))
            .unwrap()
            .next()
            .is_some()
    );

    // What an earlier build or model left is removed, as is YOLO5Face-n's folder from before its
    // batch was fixed; this build's compiled models stay.
    let other_build = dir.write("0123456789abcdef/model/coremldata.bin");
    let other_model = builds[0].join("f".repeat(64));
    std::fs::create_dir(&other_model).unwrap();
    let free_batch = builds[0].join(yolo5face::MODEL_SHA256);
    std::fs::create_dir(&free_batch).unwrap();
    let stray = dir.write("stray");
    assert_eq!(cached.remove_unused_coreml_caches().unwrap(), 4);
    assert!(!other_build.parent().unwrap().parent().unwrap().exists());
    assert!(!other_model.exists() && !free_batch.exists() && !stray.exists());
    assert!(builds[0].join(yunet::MODEL_SHA256).is_dir());
    assert_eq!(cached.remove_unused_coreml_caches().unwrap(), 0);
    let none = runtime().clone();
    assert_eq!(none.remove_unused_coreml_caches().unwrap(), 0);
}

/// With YOLO5Face-n's batch fixed, CoreML takes the whole graph: its cache holds one compiled
/// partition, `0_dynamic_mlprogram`, where a free batch leaves `0_…` and `1_…`. The folder name
/// keeps the two apart, since a fixed batch would load the free one's partial `0_…`.
#[cfg(target_os = "macos")]
#[test]
fn yolo5face_runs_as_one_coreml_partition() {
    let dir = common::TestDir::new();
    let cached = runtime()
        .clone()
        .with_coreml_cache(dir.path().to_path_buf());
    Yolo5Face::new(&cached, &model_bytes(ModelId::Yolo5FaceN), Backend::CoreMl).unwrap();
    let mut partitions = Vec::new();
    for build in std::fs::read_dir(dir.path()).unwrap() {
        for model in std::fs::read_dir(build.unwrap().path()).unwrap() {
            let model = model.unwrap();
            assert_eq!(
                model.file_name().into_string().unwrap(),
                format!("{}-batch1", yolo5face::MODEL_SHA256)
            );
            for graph in std::fs::read_dir(model.path()).unwrap() {
                for entry in std::fs::read_dir(graph.unwrap().path()).unwrap() {
                    let name = entry.unwrap().file_name().into_string().unwrap();
                    if name.ends_with("_mlprogram") {
                        partitions.push(name);
                    }
                }
            }
        }
    }
    assert_eq!(partitions.len(), 1, "{partitions:?}");
}

/// The plate graph returns at most 100 boxes and suppresses at IoU 0.45. With NMS off here, the
/// car photo tiled 3x3 gives more than 100 boxes: the graph's limits no longer apply (with them,
/// 35 come out). With the default NMS, each tile's plate is found once.
#[test]
fn the_plate_graph_limits_are_lifted() {
    let assets = required_env("CLOAKFRAME_TEST_ASSETS");
    let mut detector = on_cpu(ModelId::PlateYolov9T);
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
    assert!(found.over_limit.is_empty());

    let nms = ModelId::PlateYolov9T.default_thresholds().nms_iou();
    let plates = thresholds(KindThresholds::new(0.25, nms).unwrap());
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
            if !found.over_limit.is_empty() {
                failures.push(format!("{case}: over the limit {:?}", found.over_limit));
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
