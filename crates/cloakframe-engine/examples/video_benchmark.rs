//! Analyzes one clip of the video benchmark (docs/M5_BENCHMARK.md) the way a run does, the first
//! pass and tracking, and writes the tracks the second pass would mask as JSON.
//!
//! cargo run --release -p cloakframe-engine --example video_benchmark -- --ort-library LIB
//!     --models DIR --ffmpeg FILE --ffprobe FILE --video FILE --out FILE
//!     [--faces yolo5face-n|yunet|off] [--plates on|off] [--face-score S] [--plate-score S]
//!     [--tiles on|off] [--gpu on|off] [--detections FILE]
//!
//! `--detections` also writes the first pass's detections, exactly, as JSON lines. Given those
//! instead of a video, `--from-detections FILE --out FILE [--face-score S] [--plate-score S]
//! [--faces ...] [--plates ...]` only tracks them again, which is what CI does.

use std::io::{BufRead, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use cloakframe_core::coverage::Problem;
use cloakframe_core::detection::{DetectionKind, KindThresholds, ModelId};
use cloakframe_core::geometry::Rect;
use cloakframe_core::timeline::Rational;
use cloakframe_core::tracking::Observation;
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::DetectorHandle;
use cloakframe_engine::detect::fallback::Acceleration;
use cloakframe_engine::detect::models::{DetectionModels, FaceModel};
use cloakframe_engine::detect::runtime::OrtRuntime;
use cloakframe_engine::media::source;
use cloakframe_engine::model_files::ModelStore;
use cloakframe_engine::video::analysis::{AnalysisSettings, first_pass};
use cloakframe_engine::video::record::{Detected, Order, Record, RecordWriter, TrackRegion};
use cloakframe_engine::video::tracking::{self, KindTracking};
use serde_json::{Value, json};

#[derive(Default)]
struct Options {
    ort_library: Option<PathBuf>,
    models: Option<PathBuf>,
    ffmpeg: Option<PathBuf>,
    ffprobe: Option<PathBuf>,
    video: Option<PathBuf>,
    from_detections: Option<PathBuf>,
    detections: Option<PathBuf>,
    out: PathBuf,
    face: Option<FaceModel>,
    plates: bool,
    face_score: Option<f32>,
    plate_score: Option<f32>,
    tiles: bool,
    acceleration: Option<Acceleration>,
}

/// What tracking needs of the clip besides its detections.
struct Clip {
    rate: Rational,
    width: u32,
    height: u32,
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
    let defaults = AnalysisSettings::default();
    let mut o = Options {
        face: defaults.models.face(),
        plates: defaults.models.plates(),
        tiles: defaults.tiles,
        ..Options::default()
    };
    let mut out = None;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or(format!("{flag} needs a value"))?;
        let score = |v: &str| v.parse::<f32>().map_err(|e| format!("{flag}: {e}"));
        match flag.as_str() {
            "--ort-library" => o.ort_library = Some(PathBuf::from(value)),
            "--models" => o.models = Some(PathBuf::from(value)),
            "--ffmpeg" => o.ffmpeg = Some(PathBuf::from(value)),
            "--ffprobe" => o.ffprobe = Some(PathBuf::from(value)),
            "--video" => o.video = Some(PathBuf::from(value)),
            "--from-detections" => o.from_detections = Some(PathBuf::from(value)),
            "--detections" => o.detections = Some(PathBuf::from(value)),
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
                o.acceleration = Some(if on_off(&value)? {
                    Acceleration::Gpu
                } else {
                    Acceleration::CpuOnly
                })
            }
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    o.out = out.ok_or("--out is required")?;
    let detecting = [&o.ort_library, &o.models, &o.ffmpeg, &o.ffprobe, &o.video];
    if o.from_detections.is_none() && detecting.iter().any(|v| v.is_none()) {
        return Err(
            "--ort-library, --models, --ffmpeg, --ffprobe and --video are required to detect"
                .into(),
        );
    }
    Ok(o)
}

fn rounded(v: f32, places: i32) -> f64 {
    let scale = 10f64.powi(places);
    (f64::from(v) * scale).round() / scale
}

fn kind_name(kind: DetectionKind) -> &'static str {
    match kind {
        DetectionKind::Face => "face",
        DetectionKind::Plate => "plate",
    }
}

fn settings(o: &Options) -> Result<AnalysisSettings, String> {
    let models = DetectionModels::new(o.face, o.plates).map_err(|e| e.to_string())?;
    let defaults = models.default_thresholds();
    let with_score = |t: KindThresholds, score: Option<f32>| match score {
        Some(s) => KindThresholds::new(s, t.nms_iou()).map_err(|e| e.to_string()),
        None => Ok(t),
    };
    let mut thresholds = defaults;
    thresholds.face = with_score(defaults.face, o.face_score)?;
    thresholds.plate = with_score(defaults.plate, o.plate_score)?;
    Ok(AnalysisSettings {
        models,
        thresholds,
        tiles: o.tiles,
        ..AnalysisSettings::default()
    })
}

/// Runs the first pass of `--video` into `record` and returns the clip and the seconds it took.
fn detect(o: &Options, settings: &AnalysisSettings, record: &Path) -> Result<(Clip, f64), String> {
    let video = o.video.as_ref().unwrap();
    let source = source::open(o.ffprobe.as_ref().unwrap(), video, &CancelToken::new())
        .map_err(|e| e.to_string())?;
    let runtime = OrtRuntime::load(o.ort_library.as_ref().unwrap()).map_err(|e| e.to_string())?;
    let store = ModelStore::new(o.models.as_ref().unwrap());
    let acceleration = o.acceleration.unwrap_or(Acceleration::Gpu);
    let loaded = settings
        .models
        .load(&runtime, &store, acceleration)
        .map_err(|e| e.to_string())?;
    let detector = DetectorHandle::spawn(loaded.detector).map_err(|e| e.to_string())?;
    let started = Instant::now();
    first_pass(
        o.ffmpeg.as_ref().unwrap(),
        video,
        &source,
        &detector,
        settings,
        record,
        &CancelToken::new(),
        |_| {},
    )
    .map_err(|e| e.to_string())?;
    let seconds = started.elapsed().as_secs_f64();
    let (width, height) = source.spec.display_size();
    let clip = Clip {
        // As the video processor tracks: the average rate when ffprobe gives one.
        rate: source.spec.average_rate.unwrap_or(source.spec.rate),
        width,
        height,
    };
    Ok((clip, seconds))
}

/// The record as a head line (clip, frames, scene cuts) and a line per frame, with every value
/// as the record holds it so tracking them again gives the same tracks.
fn write_detections(record: &Record<Detected>, clip: &Clip, path: &Path) -> Result<(), String> {
    let err = |e: std::io::Error| e.to_string();
    let mut out = BufWriter::new(std::fs::File::create(path).map_err(err)?);
    let head = json!({
        "frames": record.frames(),
        "rate": [clip.rate.num(), clip.rate.den()],
        "width": clip.width,
        "height": clip.height,
        "cuts": record.cuts(),
    });
    writeln!(out, "{head}").map_err(err)?;
    let mut frames = record.ascending().map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    while let Some(frame) = frames.next_into(&mut entries).map_err(|e| e.to_string())? {
        let list: Vec<Value> = entries
            .iter()
            .map(|d| {
                let (o, b) = (&d.observation, d.observation.bbox);
                json!([
                    d.model,
                    o.score,
                    b.x(),
                    b.y(),
                    b.width(),
                    b.height(),
                    o.roll
                ])
            })
            .collect();
        writeln!(out, "{}", json!([frame, list])).map_err(err)?;
    }
    out.flush().map_err(err)
}

fn read_detections(path: &Path, record: &Path) -> Result<Clip, String> {
    let bad = |what: &str| format!("{}: {what}", path.display());
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut lines = std::io::BufReader::new(file).lines();
    let head: Value = serde_json::from_str(
        &lines
            .next()
            .ok_or_else(|| bad("empty"))?
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let int = |v: &Value| v.as_u64().ok_or_else(|| bad("not a whole number"));
    let rate = Rational::new(int(&head["rate"][0])? as u32, int(&head["rate"][1])? as u32)
        .ok_or_else(|| bad("bad rate"))?;
    let clip = Clip {
        rate,
        width: int(&head["width"])? as u32,
        height: int(&head["height"])? as u32,
    };
    let cuts: Vec<u64> = serde_json::from_value(head["cuts"].clone()).map_err(|e| e.to_string())?;
    let mut writer =
        RecordWriter::<Detected>::create(record, Order::Ascending).map_err(|e| e.to_string())?;
    let float = |v: &Value| {
        v.as_f64()
            .map(|f| f as f32)
            .ok_or_else(|| bad("not a number"))
    };
    for line in lines {
        let (frame, list): (u64, Vec<Value>) =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let mut entries = Vec::with_capacity(list.len());
        for d in &list {
            let model: ModelId = serde_json::from_value(d[0].clone()).map_err(|e| e.to_string())?;
            let bbox = Rect::new(float(&d[2])?, float(&d[3])?, float(&d[4])?, float(&d[5])?)
                .map_err(|e| e.to_string())?;
            let roll = if d[6].is_null() {
                None
            } else {
                Some(float(&d[6])?)
            };
            entries.push(Detected {
                model,
                observation: Observation {
                    bbox,
                    score: float(&d[1])?,
                    roll,
                },
            });
        }
        writer.frame(frame, &entries).map_err(|e| e.to_string())?;
    }
    writer.finish(&cuts).map_err(|e| e.to_string())?;
    Ok(clip)
}

fn run(o: &Options) -> Result<(), String> {
    let settings = settings(o)?;
    let record = o.out.with_extension("record");
    let (clip, detect_seconds) = match &o.from_detections {
        Some(path) => (read_detections(path, &record)?, None),
        None => {
            let (clip, seconds) = detect(o, &settings, &record)?;
            (clip, Some(seconds))
        }
    };
    let detected = Record::<Detected>::open(&record).map_err(|e| e.to_string())?;
    if let Some(path) = &o.detections {
        write_detections(&detected, &clip, path)?;
    }

    let kinds: Vec<KindTracking> = settings
        .kinds()
        .into_iter()
        .map(|kind| {
            KindTracking::new(
                kind,
                settings.track_score(kind),
                clip.rate,
                &settings.windows,
            )
        })
        .collect();
    let regions = o.out.with_extension("regions");
    let started = Instant::now();
    let tracked = tracking::track(
        &detected,
        &o.out.with_extension("backward"),
        &regions,
        &kinds,
        &CancelToken::new(),
    )
    .map_err(|e| e.to_string())?;
    let track_seconds = started.elapsed().as_secs_f64();

    let mut boxes: std::collections::BTreeMap<u32, Vec<Value>> = Default::default();
    let regions_record = Record::<TrackRegion>::open(&regions).map_err(|e| e.to_string())?;
    let mut frames = regions_record.ascending().map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    while let Some(frame) = frames.next_into(&mut entries).map_err(|e| e.to_string())? {
        for r in &entries {
            let b = r.bbox;
            let row = json!([
                frame,
                rounded(b.x(), 2),
                rounded(b.y(), 2),
                rounded(b.width(), 2),
                rounded(b.height(), 2),
                rounded(r.score, 4),
                u8::from(r.interpolated)
            ]);
            boxes.entry(r.track).or_default().push(row);
        }
    }
    for path in [&regions, &record] {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }

    let gaps: Vec<Value> = tracked
        .gaps
        .iter()
        .filter_map(|g| match &g.problem {
            Problem::UnresolvedGap {
                detection,
                first,
                last,
                ..
            } => Some(json!([first, last, kind_name(*detection)])),
            _ => None,
        })
        .collect();
    let mut text = serde_json::to_string(&json!({
        "detector": "engine",
        "frames": detected.frames(),
        "width": clip.width,
        "height": clip.height,
        "tiles": o.from_detections.is_none().then_some(o.tiles),
        "detect_seconds": detect_seconds,
        "track_seconds": track_seconds,
        "gaps": gaps,
    }))
    .map_err(|e| e.to_string())?;
    text.pop();
    text.push_str(",\"tracks\":[\n");
    let lines: Vec<String> = tracked
        .tracks
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "kind": kind_name(t.kind),
                "low_confidence": t.low_confidence,
                "boxes": boxes.remove(&t.id).unwrap_or_default()
            })
            .to_string()
        })
        .collect();
    text.push_str(&lines.join(",\n"));
    text.push_str("\n]}\n");
    std::fs::write(&o.out, text).map_err(|e| format!("{}: {e}", o.out.display()))
}

fn main() -> ExitCode {
    // SAFETY: the first statement of main; no other thread exists yet.
    #[allow(unsafe_code)]
    unsafe {
        cloakframe_engine::detect::runtime::disable_telemetry()
    };
    match parse().and_then(|o| run(&o)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("video_benchmark: {e}");
            ExitCode::FAILURE
        }
    }
}
