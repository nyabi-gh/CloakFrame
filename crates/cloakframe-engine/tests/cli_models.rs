//! `cloakframe-cli` and the model files: nothing is loaded before the scan finds media and only the
//! models a run detects with are read (D7), downloads from the projects' GitHub pages with their
//! terms, and the app's model folder taking over what CloakFrame 1.12.1 downloaded and compiled.
//!
//! Needs `CLOAKFRAME_ORT_LIBRARY` and `CLOAKFRAME_TEST_ASSETS` (see CONTRIBUTING.md), and the
//! network for the download test: it fetches the two MIT-licensed models from GitHub.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::{TestDir, required_env};

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

struct Fixture {
    dir: TestDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            dir: TestDir::new(),
        }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    /// The app's folders under the test's own home folder.
    fn cli(&self, args: &[&str]) -> Output {
        let output = Command::new(env!("CARGO_BIN_EXE_cloakframe-cli"))
            .args(args)
            .env("HOME", self.root().join("home"))
            .env_remove("XDG_DATA_HOME")
            .env_remove("XDG_CACHE_HOME")
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
        let out = self.root().join("out");
        let sessions = self.root().join("sessions");
        let input = self.root().join("in");
        let mut args = vec![
            "run",
            "--output",
            out.to_str().unwrap(),
            "--sessions",
            sessions.to_str().unwrap(),
            "--ort-library",
            library.to_str().unwrap(),
            "--review",
            "off",
        ];
        args.extend(extra);
        args.push(input.to_str().unwrap());
        self.cli(&args)
    }

    fn astronaut(&self) {
        self.copy_asset("astronaut.png", &self.root().join("in"));
    }

    fn copy_asset(&self, name: &str, to: &Path) -> PathBuf {
        std::fs::create_dir_all(to).unwrap();
        let path = to.join(name);
        std::fs::copy(required_env("CLOAKFRAME_TEST_ASSETS").join(name), &path).unwrap();
        path
    }
}

const FACES: &str = "yolov5n_face.onnx";
const YUNET: &str = "face_detection_yunet_2023mar.onnx";
const PLATES: &str = "yolo-v9-t-512-license-plates-end2end.onnx";

/// D7 case 1: a folder with no media and paths to nothing for the runtime and the models.
#[test]
fn nothing_is_loaded_before_the_scan_finds_media() {
    let f = Fixture::new();
    f.dir.write("in/notes.txt");
    let nowhere = f.root().join("nowhere");
    let r = f.cli(&[
        "run",
        "--output",
        f.root().join("out").to_str().unwrap(),
        "--sessions",
        f.root().join("sessions").to_str().unwrap(),
        "--ort-library",
        nowhere.join("libonnxruntime").to_str().unwrap(),
        "--models",
        nowhere.to_str().unwrap(),
        f.root().join("in").to_str().unwrap(),
    ]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(
        r.stderr
            .contains("skipped 1 file(s) of unsupported type txt"),
        "{}",
        r.stderr
    );
    assert!(!r.stderr.contains("error"), "{}", r.stderr);
    assert!(!nowhere.exists());
}

/// D7 case 3: with plates off the plate model is not read, so a folder without it is enough.
#[test]
fn the_plate_model_is_read_only_when_plates_are_on() {
    let f = Fixture::new();
    f.astronaut();
    let models = f.root().join("models");
    f.copy_asset(FACES, &models);
    let r = f.run(&["--models", models.to_str().unwrap(), "--plates", "off"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);

    let r = f.run(&["--models", models.to_str().unwrap(), "--plates", "on"]);
    assert_eq!(r.code, 1, "{}{}", r.stdout, r.stderr);
    let missing = format!(
        "the model {} is missing; download it with `cloakframe-cli models fetch plates --models DIR`",
        models.join(PLATES).display()
    );
    assert!(r.stderr.contains(&missing), "{}", r.stderr);
}

fn line_with<'a>(text: &'a str, start: &str) -> &'a str {
    text.lines()
        .find(|l| l.starts_with(start))
        .unwrap_or_else(|| panic!("no line starting with {start:?} in\n{text}"))
}

#[test]
fn models_are_downloaded_from_their_projects_with_their_terms() {
    let f = Fixture::new();
    let models = f.root().join("models");
    let dir = models.to_str().unwrap();
    let r = f.cli(&["models", "fetch", "yunet", "plates", "--models", dir]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(
        r.stderr
            .contains("From the OpenCV Zoo under the MIT license.")
    );
    for (name, file) in [("yunet", YUNET), ("plates", PLATES)] {
        let saved = format!("{name}\t{}", models.join(file).display());
        assert!(r.stdout.lines().any(|l| l == saved), "{}", r.stdout);
        let asset = std::fs::read(required_env("CLOAKFRAME_TEST_ASSETS").join(file)).unwrap();
        assert_eq!(std::fs::read(models.join(file)).unwrap(), asset);
    }
    let mut names: Vec<_> = std::fs::read_dir(&models)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, [YUNET, PLATES]);

    let r = f.cli(&["models", "--models", dir]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(
        line_with(&r.stdout, "yolo5face-n\t")
            .starts_with("yolo5face-n\tmissing\t10.5 MB\tnon-commercial")
    );
    assert!(line_with(&r.stdout, "yunet\t").starts_with("yunet\tpresent\t0.2 MB\tMIT"));
    assert!(line_with(&r.stdout, "plates\t").starts_with("plates\tpresent\t7.4 MB\tMIT"));
    assert!(
        r.stdout
            .contains("\thttps://github.com/deepcam-cn/yolov5-face\n")
    );
    assert_eq!(line_with(&r.stdout, "folder\t"), format!("folder\t{dir}"));

    let r = f.cli(&["models", "fetch", "yunet", "--models", dir]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(r.stderr.contains("YuNet is already in"), "{}", r.stderr);

    // YOLO5Face-n's terms limit its use, so it is not downloaded without --accept-terms.
    let r = f.cli(&["models", "fetch", "yolo5face-n", "yunet", "--models", dir]);
    assert_eq!(r.code, 2, "{}{}", r.stdout, r.stderr);
    assert!(
        r.stderr.contains("for non-commercial purposes only"),
        "{}",
        r.stderr
    );
    assert!(r.stderr.contains("pass --accept-terms"), "{}", r.stderr);
    assert!(!models.join(FACES).exists());

    for args in [
        &["models", "fetch", "scrfd", "--models", dir][..],
        &["models", "fetch", "--models", dir],
        &["models", "remove", "--models", dir],
    ] {
        let r = f.cli(args);
        assert_eq!(r.code, 2, "{args:?}: {}{}", r.stdout, r.stderr);
        assert!(r.stderr.contains("usage:"), "{}", r.stderr);
    }
}

/// The app's model folder is under the home folder the test sets, which Windows does not use
/// for its known folders; there the same code runs with the real local app data folder.
#[cfg(not(windows))]
#[test]
fn the_app_folder_takes_over_what_1_12_1_left() {
    let f = Fixture::new();
    f.astronaut();
    let home = f.root().join("home");
    let (data, app) = if cfg!(target_os = "macos") {
        (home.join("Library/Application Support"), "CloakFrame")
    } else {
        (home.join(".local/share"), "cloakframe")
    };
    // 1.12.1 kept its models in <data>/CloakFrame/models and still read Redactly's. On macOS the
    // first is the app's own folder; on Linux the app's folder is in lower case.
    f.copy_asset(YUNET, &data.join("Redactly/models"));
    f.copy_asset(PLATES, &data.join("CloakFrame/models"));
    let app_models = data.join(app).join("models");

    // A folder given with --models is the user's: nothing is copied into it.
    let given = f.root().join("given");
    let r = f.cli(&["models", "--models", given.to_str().unwrap()]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(!r.stderr.contains("copied"), "{}", r.stderr);
    assert!(line_with(&r.stdout, "yunet\t").starts_with("yunet\tmissing"));
    assert!(!given.exists());

    let r = f.cli(&["models"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(
        r.stderr.contains(&format!(
            "copied {YUNET} from {}",
            data.join("Redactly/models").display()
        )),
        "{}",
        r.stderr
    );
    let copied_plates = r.stderr.contains(&format!("copied {PLATES}"));
    assert_eq!(copied_plates, !cfg!(target_os = "macos"), "{}", r.stderr);
    assert!(line_with(&r.stdout, "yunet\t").starts_with("yunet\tpresent"));
    assert!(line_with(&r.stdout, "plates\t").starts_with("plates\tpresent"));
    assert_eq!(
        line_with(&r.stdout, "folder\t"),
        format!("folder\t{}", app_models.display())
    );
    // The sources stay.
    assert!(data.join("Redactly/models").join(YUNET).exists());

    let cache = home.join("Library/Caches/CloakFrame");
    let compiled_by_1_12_1 = f
        .dir
        .write("home/Library/Caches/CloakFrame/CloakFrame/coreml-models/tag/model.mlmodelc/x");
    let other_build = f
        .dir
        .write("home/Library/Caches/CloakFrame/coreml/0123456789abcdef/x");
    let r = f.run(&["--faces", "yunet", "--plates", "on"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(!r.stderr.contains("copied"), "{}", r.stderr);
    if cfg!(target_os = "macos") {
        assert!(!cache.join("CloakFrame/coreml-models").exists());
        assert!(!other_build.parent().unwrap().exists());
        assert!(cache.join("coreml").is_dir());
    } else {
        // Only macOS compiles models for CoreML; the files are left alone elsewhere.
        assert!(compiled_by_1_12_1.exists() && other_build.exists());
    }
}
