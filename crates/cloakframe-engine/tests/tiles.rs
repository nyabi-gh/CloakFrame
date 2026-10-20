//! Tile mode on the real face models: faces too small for the whole view are found and masked
//! whole, also where the finest tiles cut them.
//!
//! Needs `CLOAKFRAME_ORT_LIBRARY`, `CLOAKFRAME_TEST_ASSETS` and `ORT_DISABLE_TELEMETRY=1`, as
//! detect_fixtures.rs does.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use cloakframe_core::detection::Detection;
use cloakframe_core::geometry::Rect;
use cloakframe_core::item::{ItemId, ItemState, PublishBasis, ReviewMode};
use cloakframe_engine::detect::models::{DetectionModels, FaceModel};
use cloakframe_engine::detect::runtime::{Backend, OrtRuntime};
use cloakframe_engine::detect::yolo5face::Yolo5Face;
use cloakframe_engine::detect::yunet::YuNet;
use cloakframe_engine::detect::{Detector, DetectorHandle};
use cloakframe_engine::photo::process::{PhotoProcessor, PhotoSettings};
use cloakframe_engine::run::{Job, JobItem, RunReport, Start, start};
use common::{TestDir, fixtures, required_env};
use image::{Rgb, RgbImage, imageops};

/// ORT has one environment per process, shared by the tests here.
fn runtime() -> &'static OrtRuntime {
    static RUNTIME: OnceLock<OrtRuntime> = OnceLock::new();
    RUNTIME.get_or_init(|| OrtRuntime::load(&required_env("CLOAKFRAME_ORT_LIBRARY")).unwrap())
}

fn assets() -> PathBuf {
    required_env("CLOAKFRAME_TEST_ASSETS")
}

fn detector(face: FaceModel) -> Box<dyn Detector> {
    match face {
        FaceModel::Yolo5FaceN => {
            let model = std::fs::read(assets().join("yolov5n_face.onnx")).unwrap();
            Box::new(Yolo5Face::new(runtime(), &model, Backend::Cpu).unwrap())
        }
        FaceModel::YuNet => {
            let model = std::fs::read(assets().join("face_detection_yunet_2023mar.onnx")).unwrap();
            Box::new(YuNet::new(runtime(), &model, Backend::Cpu).unwrap())
        }
    }
}

fn run(input: &Path, destination: &Path, face: FaceModel, tiles: bool) -> RunReport {
    let models = DetectionModels::new(Some(face), false).unwrap();
    let settings = PhotoSettings {
        models,
        thresholds: models.default_thresholds(),
        tiles,
        ..PhotoSettings::default()
    };
    let job = Job {
        items: vec![JobItem {
            id: ItemId(0),
            input: input.to_owned(),
            destination: destination.to_owned(),
            start: Start::Fresh {
                needs_review: false,
            },
        }],
        review: ReviewMode::Off,
        workers: 1,
    };
    let processor = PhotoProcessor::new(settings).unwrap();
    let handle = DetectorHandle::spawn(detector(face)).unwrap();
    let report = start(job, Arc::new(processor), handle).unwrap().wait();
    assert!(
        matches!(report.items[0].state, ItemState::Published(_)),
        "{:?}",
        report.items[0].failure
    );
    report
}

/// Whether some detection overlaps `face` as a box of the same face would.
fn found(detections: &[Detection], face: &Rect) -> bool {
    detections.iter().any(|d| d.bbox().iou(face) >= 0.3)
}

/// Whether the middle of `face` (all but a tenth on each side) is the black fill.
fn masked_whole(out: &RgbImage, face: &Rect) -> bool {
    let (mx, my) = (face.width() * 0.1, face.height() * 0.1);
    let xs = (face.x() + mx) as u32..(face.right() - mx) as u32;
    let ys = (face.y() + my) as u32..(face.bottom() - my) as u32;
    ys.flat_map(|y| xs.clone().map(move |x| (x, y)))
        .all(|(x, y)| out.get_pixel(x, y).0 == [0, 0, 0])
}

/// The five astronauts of the fixture (512 to 32 pixels), as boxes around their faces.
fn astronaut_faces() -> Vec<Rect> {
    let mut x = 16.0;
    [512.0, 256.0, 128.0, 64.0, 32.0]
        .into_iter()
        .map(|size: f32| {
            let s = size / 512.0;
            let face = Rect::new(x + 170.0 * s, 16.0 + 40.0 * s, 110.0 * s, 150.0 * s).unwrap();
            x += size + 16.0;
            face
        })
        .collect()
}

#[test]
fn tiles_find_the_astronauts_the_whole_view_misses() {
    let input = fixtures().join("detect/inputs/astronaut-scales.png");
    let faces = astronaut_faces();
    for face in [FaceModel::Yolo5FaceN, FaceModel::YuNet] {
        let out = TestDir::new();
        let whole = run(&input, &out.path().join("whole.png"), face, false);
        let tiled = run(&input, &out.path().join("tiled.png"), face, true);
        let found_by = |r: &RunReport| -> Vec<bool> {
            let detections = &r.items[0].analysis.detections;
            faces.iter().map(|f| found(detections, f)).collect()
        };
        let (whole, tiled) = (found_by(&whole), found_by(&tiled));
        // The tiles keep what the whole view finds and add the 64-pixel astronaut.
        assert!(
            whole.iter().zip(&tiled).all(|(&w, &t)| t || !w),
            "{face:?}: {whole:?} -> {tiled:?}"
        );
        assert!(!whole[3] && tiled[3], "{face:?}: {whole:?} -> {tiled:?}");
    }
}

/// A gray 4000x3000 photo with the astronaut's face at `(centre, height)` places.
fn photo_with_faces(dir: &TestDir, faces: &[((u32, u32), u32)]) -> (PathBuf, Vec<Rect>) {
    let astronaut = image::open(assets().join("astronaut.png"))
        .unwrap()
        .to_rgb8();
    let crop = imageops::crop_imm(&astronaut, 170, 40, 110, 150).to_image();
    let mut photo = RgbImage::from_pixel(4000, 3000, Rgb([96, 96, 96]));
    let mut boxes = Vec::new();
    for &((cx, cy), height) in faces {
        let width = height * 110 / 150;
        let face = imageops::resize(&crop, width, height, imageops::FilterType::Lanczos3);
        let (x, y) = (cx - width / 2, cy - height / 2);
        imageops::replace(&mut photo, &face, i64::from(x), i64::from(y));
        boxes.push(Rect::new(x as f32, y as f32, width as f32, height as f32).unwrap());
    }
    let path = dir.path().join("faces.png");
    photo.save(&path).unwrap();
    (path, boxes)
}

#[test]
fn faces_where_the_finest_tiles_meet_are_masked_whole() {
    // The finest level's tiles start every 480 pixels across and every 472 down, so these
    // centres sit in the middle of their overlaps. The 300-pixel face is wider than an overlap:
    // every finest tile it touches cuts it.
    let dir = TestDir::new();
    let (input, faces) = photo_with_faces(
        &dir,
        &[
            ((560, 556), 96),
            ((1520, 1028), 128),
            ((2480, 1500), 300),
            ((3000, 2300), 64),
        ],
    );
    for model in [FaceModel::Yolo5FaceN, FaceModel::YuNet] {
        let destination = dir.path().join("out.png");
        let whole = run(&input, &destination, model, false);
        let detections = &whole.items[0].analysis.detections;
        for (i, face) in faces.iter().enumerate() {
            if face.height() < 200.0 {
                assert!(
                    !found(detections, face),
                    "{model:?}: whole view found face {i}"
                );
            }
        }
        std::fs::remove_file(&destination).unwrap();

        let tiled = run(&input, &destination, model, true);
        let outcome = &tiled.items[0];
        assert_eq!(
            outcome.state,
            ItemState::Published(PublishBasis::AutomaticUnreviewed)
        );
        let out = image::open(&destination).unwrap().to_rgb8();
        for (i, face) in faces.iter().enumerate() {
            let on_it: Vec<&Detection> = outcome
                .analysis
                .detections
                .iter()
                .filter(|d| d.bbox().iou(face) > 0.0)
                .collect();
            assert_eq!(on_it.len(), 1, "{model:?}: face {i} has {on_it:?}");
            assert!(masked_whole(&out, face), "{model:?}: face {i} {on_it:?}");
        }
        std::fs::remove_file(&destination).unwrap();
    }
}
