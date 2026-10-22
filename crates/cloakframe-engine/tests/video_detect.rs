//! A video's first pass with the pinned models. Needs `CLOAKFRAME_ORT_LIBRARY`,
//! `CLOAKFRAME_TEST_ASSETS`, `ORT_DISABLE_TELEMETRY=1`, `CLOAKFRAME_FFMPEG` and
//! `CLOAKFRAME_FFPROBE`.

mod common;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::DetectorHandle;
use cloakframe_engine::detect::fallback::Acceleration;
use cloakframe_engine::detect::models::DetectionModels;
use cloakframe_engine::detect::runtime::OrtRuntime;
use cloakframe_engine::media::source::{self, Source};
use cloakframe_engine::model_files::ModelStore;
use cloakframe_engine::video::analysis::{AnalysisSettings, first_pass};
use common::{TestDir, required_env};

fn runtime() -> &'static OrtRuntime {
    static RUNTIME: OnceLock<OrtRuntime> = OnceLock::new();
    RUNTIME.get_or_init(|| OrtRuntime::load(&required_env("CLOAKFRAME_ORT_LIBRARY")).unwrap())
}

/// Five seconds at 30 frames per second of a photo of people, at 1080p.
fn clip(dir: &TestDir) -> PathBuf {
    let path = dir.path().join("people.mp4");
    let status = std::process::Command::new(required_env("CLOAKFRAME_FFMPEG"))
        .args(["-v", "error", "-loop", "1", "-i"])
        .arg(required_env("CLOAKFRAME_TEST_ASSETS").join("messi5.jpg"))
        .args(["-vf", "scale=1920:1080,setsar=1", "-t", "5", "-r", "30"])
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    path
}

/// Runs the first pass on the CPU and returns the frames and seconds it took.
fn time_first_pass(
    input: &Path,
    source: &Source,
    (spinning, tiles): (bool, bool),
    record: &Path,
) -> (u64, f64) {
    let models = DetectionModels::default();
    let runtime = runtime().clone().with_cpu_spinning(spinning);
    let store = ModelStore::new(required_env("CLOAKFRAME_TEST_ASSETS"));
    let loaded = models
        .load(&runtime, &store, Acceleration::CpuOnly)
        .unwrap();
    let detector = DetectorHandle::spawn(loaded.detector).unwrap();
    let settings = AnalysisSettings {
        tiles,
        ..AnalysisSettings::default()
    };
    let start = Instant::now();
    let pass = first_pass(
        &required_env("CLOAKFRAME_FFMPEG"),
        input,
        source,
        &detector,
        &settings,
        record,
        &CancelToken::new(),
        |_| {},
    )
    .unwrap();
    (pass.frames, start.elapsed().as_secs_f64())
}

/// Plan decision (M5): ORT's CPU sessions do not spin, measured against spinning on every OS.
/// Run with `--ignored --nocapture`.
#[test]
#[ignore = "a measurement, run on its own"]
fn first_pass_throughput_1080p() {
    let dir = TestDir::new();
    let input = clip(&dir);
    let source = source::open(
        &required_env("CLOAKFRAME_FFPROBE"),
        &input,
        &CancelToken::new(),
    )
    .unwrap();
    // Off first, then on, then off again: the first run also warms the caches.
    let runs = [(false, false), (true, false), (false, false), (false, true)];
    for (i, (spinning, tiles)) in runs.into_iter().enumerate() {
        let record = dir.path().join(format!("record-{i}"));
        let (frames, seconds) = time_first_pass(&input, &source, (spinning, tiles), &record);
        println!(
            "first pass 1080p YOLO5Face-n CPU, spinning {}, tiles {}: {frames} frames in {seconds:.2} s, {:.1} fps",
            if spinning { "on" } else { "off" },
            if tiles { "on" } else { "off" },
            frames as f64 / seconds
        );
    }
}
