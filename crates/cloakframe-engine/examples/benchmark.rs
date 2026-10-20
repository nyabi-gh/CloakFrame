//! Analyzes the detection benchmark's images (docs/M3_BENCHMARK.md) the way a run does, holding
//! every item for review so nothing is rendered, and writes the detections as JSON for
//! tools/benchmark/evaluate.py.
//!
//! cargo run --release -p cloakframe-engine --example benchmark -- --ort-library LIB --models DIR
//!     --manifest FILE --images DIR --out FILE [--faces yolo5face-n|yunet|off] [--plates on|off]
//!     [--face-score S] [--plate-score S] [--tiles on|off] [--gpu on|off] [--workers N]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use cloakframe_core::detection::{DetectionKind, KindThresholds, ModelId};
use cloakframe_core::item::{ItemId, ReviewMode};
use cloakframe_engine::detect::fallback::Acceleration;
use cloakframe_engine::detect::models::{DetectionModels, FaceModel};
use cloakframe_engine::detect::runtime::OrtRuntime;
use cloakframe_engine::detect::{DetectorHandle, tiles};
use cloakframe_engine::model_files::ModelStore;
use cloakframe_engine::photo::process::{PhotoProcessor, PhotoSettings};
use cloakframe_engine::run::{self, Job, JobItem, Start};
use serde_json::{Value, json};

struct Options {
    ort_library: PathBuf,
    models: PathBuf,
    manifest: PathBuf,
    images: PathBuf,
    out: PathBuf,
    face: Option<FaceModel>,
    plates: bool,
    face_score: Option<f32>,
    plate_score: Option<f32>,
    tiles: bool,
    acceleration: Acceleration,
    workers: usize,
}

fn on_off(value: &str) -> Result<bool, String> {
    match value {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!("expected on or off, not {value}")),
    }
}

fn parse() -> Result<Options, String> {
    let mut args = std::env::args().skip(1);
    let (mut ort_library, mut models, mut manifest, mut images, mut out) =
        (None, None, None, None, None);
    let mut o = Options {
        ort_library: PathBuf::new(),
        models: PathBuf::new(),
        manifest: PathBuf::new(),
        images: PathBuf::new(),
        out: PathBuf::new(),
        face: DetectionModels::default().face(),
        plates: DetectionModels::default().plates(),
        face_score: None,
        plate_score: None,
        tiles: PhotoSettings::default().tiles,
        acceleration: Acceleration::Gpu,
        workers: std::thread::available_parallelism().map_or(1, |n| n.get()),
    };
    while let Some(flag) = args.next() {
        let value = args.next().ok_or(format!("{flag} needs a value"))?;
        let score = |v: &str| v.parse::<f32>().map_err(|e| format!("{flag}: {e}"));
        match flag.as_str() {
            "--ort-library" => ort_library = Some(PathBuf::from(value)),
            "--models" => models = Some(PathBuf::from(value)),
            "--manifest" => manifest = Some(PathBuf::from(value)),
            "--images" => images = Some(PathBuf::from(value)),
            "--out" => out = Some(PathBuf::from(value)),
            "--faces" => {
                o.face = match value.as_str() {
                    "yolo5face-n" => Some(FaceModel::Yolo5FaceN),
                    "yunet" => Some(FaceModel::YuNet),
                    "off" => None,
                    _ => return Err(format!("unknown face model {value}")),
                }
            }
            "--plates" => o.plates = on_off(&value)?,
            "--face-score" => o.face_score = Some(score(&value)?),
            "--plate-score" => o.plate_score = Some(score(&value)?),
            "--tiles" => o.tiles = on_off(&value)?,
            "--gpu" => {
                o.acceleration = if on_off(&value)? {
                    Acceleration::Gpu
                } else {
                    Acceleration::CpuOnly
                }
            }
            "--workers" => o.workers = value.parse().map_err(|e| format!("--workers: {e}"))?,
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    let required = |v: Option<PathBuf>, name: &str| v.ok_or(format!("{name} is required"));
    o.ort_library = required(ort_library, "--ort-library")?;
    o.models = required(models, "--models")?;
    o.manifest = required(manifest, "--manifest")?;
    o.images = required(images, "--images")?;
    o.out = required(out, "--out")?;
    Ok(o)
}

fn model_name(model: ModelId) -> &'static str {
    match model {
        ModelId::Yolo5FaceN => "yolo5face-n",
        ModelId::YuNet => "yunet",
        ModelId::PlateYolov9T => "plate",
    }
}

fn with_score(t: KindThresholds, score: Option<f32>) -> Result<KindThresholds, String> {
    match score {
        Some(s) => KindThresholds::new(s, t.nms_iou()).map_err(|e| e.to_string()),
        None => Ok(t),
    }
}

fn rounded(v: f32, places: i32) -> f64 {
    let scale = 10f64.powi(places);
    (f64::from(v) * scale).round() / scale
}

/// The result with one image per line, so a recorded result stays small and diffs by image.
fn by_line(mut value: Value) -> Result<String, String> {
    let images = value
        .as_object_mut()
        .and_then(|m| m.remove("images"))
        .ok_or("no images")?;
    let mut text = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    text.pop();
    text.push_str(",\"images\":[\n");
    let lines: Result<Vec<String>, _> = images
        .as_array()
        .ok_or("no images")?
        .iter()
        .map(serde_json::to_string)
        .collect();
    text.push_str(&lines.map_err(|e| e.to_string())?.join(",\n"));
    text.push_str("\n]}\n");
    Ok(text)
}

fn benchmark(o: &Options) -> Result<Value, String> {
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(&o.manifest).map_err(|e| format!("{}: {e}", o.manifest.display()))?,
    )
    .map_err(|e| format!("{}: {e}", o.manifest.display()))?;
    let images = manifest["images"]
        .as_array()
        .ok_or("the manifest has no images")?;

    let models = DetectionModels::new(o.face, o.plates).map_err(|e| e.to_string())?;
    let defaults = models.default_thresholds();
    let mut thresholds = defaults;
    thresholds.face = with_score(defaults.face, o.face_score)?;
    thresholds.plate = with_score(defaults.plate, o.plate_score)?;
    let processor = PhotoProcessor::new(PhotoSettings {
        models,
        thresholds,
        tiles: o.tiles,
        ..PhotoSettings::default()
    })
    .map_err(|e| e.to_string())?;
    let runtime = OrtRuntime::load(&o.ort_library).map_err(|e| e.to_string())?;
    let loaded = models
        .load(&runtime, &ModelStore::new(&o.models), o.acceleration)
        .map_err(|e| e.to_string())?;
    let backends: Vec<String> = loaded
        .backends
        .get()
        .iter()
        .map(|m| format!("{} on {}", model_name(m.model), m.backend))
        .collect();
    let detector = DetectorHandle::spawn(loaded.detector).map_err(|e| e.to_string())?;

    let never_written = std::env::temp_dir().join("cloakframe-benchmark-no-output");
    let mut items = Vec::with_capacity(images.len());
    for (i, image) in images.iter().enumerate() {
        let name = image["name"].as_str().ok_or("an image without a name")?;
        items.push(JobItem {
            id: ItemId(i as u64),
            input: o.images.join(name),
            destination: never_written.join(name),
            start: Start::Fresh { needs_review: true },
        });
    }
    let job = Job {
        items,
        review: ReviewMode::Required,
        workers: o.workers,
    };
    let started = Instant::now();
    let report = run::start(job, Arc::new(processor), detector)
        .map_err(|e| e.to_string())?
        .wait();
    let seconds = started.elapsed().as_secs_f64();

    let mut results = Vec::with_capacity(report.items.len());
    for (outcome, image) in report.items.iter().zip(images) {
        let size = &image["size"];
        let (w, h) = (
            size[0].as_u64().unwrap_or(0) as u32,
            size[1].as_u64().unwrap_or(0) as u32,
        );
        let views = if o.tiles {
            tiles::layout(w, h).len() + 1
        } else {
            1
        };
        let detections: Vec<Value> = outcome
            .analysis
            .detections
            .iter()
            .map(|d| {
                let b = d.bbox();
                let bbox = [b.x(), b.y(), b.width(), b.height()].map(|v| rounded(v, 1));
                json!({
                    "kind": match d.kind() {
                        DetectionKind::Face => "face",
                        DetectionKind::Plate => "plate",
                    },
                    "model": model_name(d.model()),
                    "score": rounded(d.score(), 4),
                    "box": bbox,
                })
            })
            .collect();
        results.push(json!({
            "name": image["name"],
            "failure": outcome.failure,
            "views": views,
            "unresolved": outcome.analysis.evidence.len(),
            "detections": detections,
        }));
    }
    let t =
        |k: KindThresholds| json!({"score": rounded(k.score(), 4), "nms": rounded(k.nms_iou(), 4)});
    Ok(json!({
        "detector": "engine",
        "faces": o.face.map(|f| model_name(f.id())),
        "plates": o.plates,
        "thresholds": {"face": t(thresholds.face), "plate": t(thresholds.plate)},
        "tiles": o.tiles,
        "backends": backends,
        "workers": o.workers,
        "seconds": seconds,
        "images": results,
    }))
}

fn main() -> ExitCode {
    // SAFETY: the first statement of main; no other thread exists yet.
    #[allow(unsafe_code)]
    unsafe {
        cloakframe_engine::detect::runtime::disable_telemetry()
    };
    let result = parse().and_then(|o| {
        let text = by_line(benchmark(&o)?)?;
        std::fs::write(&o.out, text).map_err(|e| format!("{}: {e}", o.out.display()))
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("benchmark: {e}");
            ExitCode::FAILURE
        }
    }
}
