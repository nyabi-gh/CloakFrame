//! `cloakframe-cli` on photos with the real detectors: an automatic run, faces and plates in one
//! run, a review that excludes a detection and adds an area, exporting without anonymization,
//! decisions refused for other settings or models, the pinned models, and a rerun that leaves the
//! earlier output alone (1.12.1 testWorkerReportsUnredactedOutputAsWarningAndPreservesIt).
//!
//! Needs `CLOAKFRAME_ORT_LIBRARY` and `CLOAKFRAME_TEST_ASSETS` (see CONTRIBUTING.md). The CLI sets
//! `ORT_DISABLE_TELEMETRY` itself, so the tests run it without that variable.

mod common;

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{TestDir, fixtures, required_env, rgb};
use image::{Rgb, RgbImage};
use serde::Deserialize;

fn assets() -> PathBuf {
    required_env("CLOAKFRAME_TEST_ASSETS")
}

struct Fixture {
    root: PathBuf,
    _dir: TestDir,
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Fixture {
    fn new() -> Self {
        let dir = TestDir::new();
        let root = dir.mkdir("in").parent().unwrap().to_path_buf();
        Self { root, _dir: dir }
    }

    fn input(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join("in").join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn astronaut(&self) -> PathBuf {
        self.input(
            "astronaut.png",
            &std::fs::read(assets().join("astronaut.png")).unwrap(),
        )
    }

    fn out(&self, name: &str) -> PathBuf {
        self.root.join("out").join(name)
    }

    fn cli(&self, args: &[&str]) -> Output {
        let output = Command::new(env!("CARGO_BIN_EXE_cloakframe-cli"))
            .args(args)
            .arg("--sessions")
            .arg(self.root.join("sessions"))
            .env_remove("ORT_DISABLE_TELEMETRY")
            .output()
            .unwrap();
        Output {
            code: output.status.code().unwrap(),
            stdout: String::from_utf8(output.stdout).unwrap(),
            stderr: String::from_utf8(output.stderr).unwrap(),
        }
    }

    fn run(&self, extra: &[&str]) -> Output {
        self.run_with_models(&assets(), extra)
    }

    fn run_with_models(&self, models: &Path, extra: &[&str]) -> Output {
        let library = required_env("CLOAKFRAME_ORT_LIBRARY");
        let out = self.root.join("out");
        let input = self.root.join("in");
        let mut args = vec![
            "run",
            "--output",
            out.to_str().unwrap(),
            "--ort-library",
            library.to_str().unwrap(),
            "--models",
            models.to_str().unwrap(),
        ];
        args.extend(extra);
        args.push(input.to_str().unwrap());
        self.cli(&args)
    }

    fn review(&self, session: &str, decisions: &str, extra: &[&str]) -> Output {
        let path = self.root.join("decisions.json");
        std::fs::write(&path, decisions).unwrap();
        let mut args = vec![
            "review",
            "--session",
            session,
            "--decisions",
            path.to_str().unwrap(),
        ];
        args.extend(extra);
        self.cli(&args)
    }

    fn session(&self, id: &str) -> serde_json::Value {
        let path = self.root.join("sessions").join(format!("{id}.json"));
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }
}

fn session_id(stdout: &str) -> String {
    stdout
        .lines()
        .find_map(|l| l.strip_prefix("session\t"))
        .expect("a session was saved")
        .to_owned()
}

#[derive(Deserialize)]
struct Expected {
    detections: Vec<ExpectedDetection>,
}

#[derive(Deserialize)]
struct ExpectedDetection {
    #[serde(rename = "box")]
    bbox: [f32; 4],
}

/// The face 1.12.1 found in astronaut.png at the default score threshold.
fn astronaut_face() -> [f32; 4] {
    let path = fixtures().join("detect/yolo5face/astronaut@0.50.json");
    let want: Expected = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(want.detections.len(), 1);
    want.detections[0].bbox
}

fn centre(b: [f32; 4]) -> (u32, u32) {
    ((b[0] + b[2] / 2.0) as u32, (b[1] + b[3] / 2.0) as u32)
}

const BLACK: [u8; 3] = [0, 0, 0];

#[test]
fn an_automatic_run_masks_the_faces_it_finds() {
    let f = Fixture::new();
    let source = f.astronaut();
    let r = f.run(&["--review", "off"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(r.stdout.contains("published (automatic, unreviewed)"));
    let (x, y) = centre(astronaut_face());
    let out = rgb(&f.out("astronaut.png"));
    assert_eq!(out.get_pixel(x, y).0, BLACK);
    assert_eq!(out.get_pixel(400, 450), rgb(&source).get_pixel(400, 450));
}

#[test]
fn a_review_can_exclude_a_detection_and_add_an_area() {
    let f = Fixture::new();
    let source = rgb(&f.astronaut());
    let r = f.run(&["--review", "required"]);
    assert_eq!(r.code, 3, "{}{}", r.stdout, r.stderr);
    assert!(!f.root.join("out").exists());
    let id = session_id(&r.stdout);
    let stored = f.session(&id);
    let detections = stored["items"][0]["detections"].as_array().unwrap();
    assert_eq!(detections.len(), 1);
    assert_eq!(detections[0]["kind"], "face");

    let r = f.review(
        &id,
        r#"{"items": [{"item": 0, "revision": 0, "action": "approve", "excluded": [0],
                       "added": [{"x": 400, "y": 400, "width": 40, "height": 40}]}]}"#,
        &[],
    );
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(r.stdout.contains("published (reviewed)"));
    let out = rgb(&f.out("astronaut.png"));
    let (x, y) = centre(astronaut_face());
    assert_eq!(out.get_pixel(x, y), source.get_pixel(x, y), "excluded");
    assert_eq!(out.get_pixel(420, 420).0, BLACK, "added");
}

#[test]
fn exporting_without_anonymization_keeps_every_pixel() {
    let f = Fixture::new();
    let source = rgb(&f.astronaut());
    let r = f.run(&["--review", "required"]);
    let id = session_id(&r.stdout);
    let r = f.review(
        &id,
        r#"{"items": [{"item": 0, "revision": 0, "action": "export_without_anonymization"}]}"#,
        &[],
    );
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(r.stdout.contains("exported without anonymization"));
    assert_eq!(rgb(&f.out("astronaut.png")), source);
}

#[test]
fn decisions_need_the_settings_the_run_used() {
    let f = Fixture::new();
    f.astronaut();
    let r = f.run(&["--review", "required", "--keep-metadata"]);
    let id = session_id(&r.stdout);
    let decision = r#"{"items": [{"item": 0, "revision": 0, "action": "approve"}]}"#;
    let r = f.review(&id, decision, &[]);
    assert_eq!(r.code, 1, "{}{}", r.stdout, r.stderr);
    assert!(
        r.stdout.contains("models or settings differ"),
        "{}",
        r.stdout
    );
    assert!(!f.root.join("out").exists());

    let r = f.review(&id, decision, &["--keep-metadata"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
}

#[test]
fn decisions_need_the_models_the_run_used() {
    let f = Fixture::new();
    f.astronaut();
    let r = f.run(&["--review", "required", "--faces", "yunet"]);
    assert_eq!(r.code, 3, "{}{}", r.stdout, r.stderr);
    let id = session_id(&r.stdout);
    let decision = r#"{"items": [{"item": 0, "revision": 0, "action": "approve"}]}"#;
    for other in [&[][..], &["--faces", "yunet", "--plates", "on"]] {
        let r = f.review(&id, decision, other);
        assert_eq!(r.code, 1, "{other:?}: {}{}", r.stdout, r.stderr);
        assert!(
            r.stdout.contains("models or settings differ"),
            "{}",
            r.stdout
        );
    }
    let r = f.review(&id, decision, &["--faces", "yunet"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
}

/// Plates are off unless asked for; with them on, the car photo's plate is masked along with
/// what the face model finds, and the result records each detection's kind (P-2).
#[test]
fn plates_join_the_faces_when_turned_on() {
    let f = Fixture::new();
    f.input(
        "car.webp",
        &std::fs::read(assets().join("car_image.webp")).unwrap(),
    );
    let plate = {
        let path = fixtures().join("detect/plate/car_image@0.50.json");
        let want: Expected = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(want.detections.len(), 1);
        want.detections[0].bbox
    };
    let kinds = |r: &Output| {
        let stored = f.session(&session_id(&r.stdout));
        let mut kinds: Vec<String> = stored["items"][0]["detections"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["kind"].as_str().unwrap().to_owned())
            .collect();
        kinds.dedup();
        kinds
    };
    let r = f.run(&["--review", "required"]);
    assert_eq!(r.code, 3, "{}{}", r.stdout, r.stderr);
    assert_eq!(kinds(&r), ["face"]);

    let r = f.run(&["--review", "required", "--plates", "on"]);
    assert_eq!(r.code, 3, "{}{}", r.stdout, r.stderr);
    assert_eq!(kinds(&r), ["face", "plate"]);
    let r = f.run(&["--review", "required", "--faces", "off", "--plates", "on"]);
    assert_eq!(r.code, 3, "{}{}", r.stdout, r.stderr);
    assert_eq!(kinds(&r), ["plate"]);

    let r = f.run(&["--review", "off", "--plates", "on"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let source = rgb(&f.root.join("in/car.webp"));
    let out = rgb(&f.out("car.webp"));
    let (x, y) = centre(plate);
    assert_ne!(source.get_pixel(x, y).0, BLACK);
    assert_eq!(out.get_pixel(x, y).0, BLACK);
}

#[test]
fn only_the_pinned_models_run() {
    let f = Fixture::new();
    f.astronaut();
    let models = f.root.join("models");
    std::fs::create_dir(&models).unwrap();
    std::fs::write(models.join("yolov5n_face.onnx"), b"not the model").unwrap();
    let out = f.root.join("out");
    let r = f.run_with_models(&models, &[]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("not the pinned one"), "{}", r.stderr);
    assert!(!r.stderr.contains("usage:"), "{}", r.stderr);
    assert!(!out.exists());
    // The YuNet file is missing.
    let r = f.run_with_models(&models, &["--faces", "yunet"]);
    assert_eq!(r.code, 1);
    assert!(
        r.stderr.contains("face_detection_yunet_2023mar.onnx"),
        "{}",
        r.stderr
    );
    assert!(!out.exists());

    let r = f.cli(&["run", "--output", out.to_str().unwrap(), "in"]);
    assert_eq!(r.code, 2);
    assert!(r.stderr.contains("--ort-library"), "{}", r.stderr);
    let r = f.cli(&["run", "--output", "out", "--videos", "in"]);
    assert_eq!(r.code, 2);
    assert!(r.stderr.contains("videos"), "{}", r.stderr);
    let r = f.run(&["--faces", "off"]);
    assert_eq!(r.code, 2);
    assert!(r.stderr.contains("--plates on"), "{}", r.stderr);
    let r = f.run(&["--faces", "scrfd"]);
    assert_eq!(r.code, 2);
    assert!(
        r.stderr.contains("yolo5face-n, yunet or off"),
        "{}",
        r.stderr
    );
    assert!(!out.exists());
}

/// A photo without faces is published like any other: no detection is not an unresolved problem
/// (1.12.1 warned), but the result notes it. A second run does not replace the first output.
#[test]
fn a_rerun_keeps_the_earlier_output() {
    let f = Fixture::new();
    let mut bytes = Cursor::new(Vec::new());
    RgbImage::from_pixel(48, 32, Rgb([60, 40, 20]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    f.input("input.png", bytes.get_ref());
    let r = f.run(&["--review", "off"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(r.stdout.contains("published (automatic, unreviewed)"));
    // Published, but said: a missed small face would otherwise go unnoticed (review item 14).
    assert!(r.stdout.contains("nothing detected"), "{}", r.stdout);
    let first = std::fs::read(f.out("input.png")).unwrap();

    let r = f.run(&["--review", "off"]);
    assert_eq!(r.code, 3, "{}{}", r.stdout, r.stderr);
    assert!(r.stdout.contains("needs review"));
    assert_eq!(std::fs::read(f.out("input.png")).unwrap(), first);
}
