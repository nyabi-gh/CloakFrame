//! `cloakframe-cli` on photos with the real detector: an automatic run, a review that excludes a
//! detection and adds an area, exporting without anonymization, decisions refused for other
//! settings, the pinned model, and a rerun that leaves the earlier output alone (1.12.1
//! testWorkerReportsUnredactedOutputAsWarningAndPreservesIt).
//!
//! Needs `CLOAKFRAME_ORT_LIBRARY` and `CLOAKFRAME_TEST_ASSETS` (see CONTRIBUTING.md). The CLI sets
//! `ORT_DISABLE_TELEMETRY` itself, so the tests run it without that variable.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use image::{Rgb, RgbImage};
use serde::Deserialize;

fn required_env(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{name} is not set (see CONTRIBUTING.md)"))
}

fn assets() -> PathBuf {
    required_env("CLOAKFRAME_TEST_ASSETS")
}

struct Fixture {
    root: PathBuf,
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let root = std::env::temp_dir().join(format!(
            "cloakframe-cli-photo-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("in")).unwrap();
        Self { root }
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
        let library = required_env("CLOAKFRAME_ORT_LIBRARY");
        let model = assets().join("yolov5n_face.onnx");
        let out = self.root.join("out");
        let input = self.root.join("in");
        let mut args = vec![
            "run",
            "--output",
            out.to_str().unwrap(),
            "--ort-library",
            library.to_str().unwrap(),
            "--face-model",
            model.to_str().unwrap(),
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

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn session_id(stdout: &str) -> String {
    stdout
        .lines()
        .find_map(|l| l.strip_prefix("session\t"))
        .expect("a session was saved")
        .to_owned()
}

fn rgb(path: &Path) -> RgbImage {
    image::open(path).unwrap().to_rgb8()
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
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/detect/yolo5face/astronaut@0.50.json");
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
fn only_the_pinned_model_runs() {
    let f = Fixture::new();
    f.astronaut();
    let other = f.input("other.onnx", b"not the model");
    let library = required_env("CLOAKFRAME_ORT_LIBRARY");
    let out = f.root.join("out");
    let r = f.cli(&[
        "run",
        "--output",
        out.to_str().unwrap(),
        "--ort-library",
        library.to_str().unwrap(),
        "--face-model",
        other.to_str().unwrap(),
        f.root.join("in/astronaut.png").to_str().unwrap(),
    ]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("not the pinned one"), "{}", r.stderr);
    assert!(!r.stderr.contains("usage:"), "{}", r.stderr);
    assert!(!out.exists());

    let r = f.cli(&["run", "--output", out.to_str().unwrap(), "in"]);
    assert_eq!(r.code, 2);
    assert!(r.stderr.contains("--ort-library"), "{}", r.stderr);
    let r = f.cli(&["run", "--output", "out", "--videos", "in"]);
    assert_eq!(r.code, 2);
    assert!(r.stderr.contains("videos"), "{}", r.stderr);
}

/// A photo without faces is published like any other: no detection is not an unresolved problem
/// (1.12.1 warned). A second run does not replace the first output.
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
    let first = std::fs::read(f.out("input.png")).unwrap();

    let r = f.run(&["--review", "off"]);
    assert_eq!(r.code, 3, "{}{}", r.stdout, r.stderr);
    assert!(r.stdout.contains("needs review"));
    assert_eq!(std::fs::read(f.out("input.png")).unwrap(), first);
}
