use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::SystemTime;

use cloakframe_core::coverage::{CoverageEvidence, Problem};
use cloakframe_core::detection::ModelId;
use cloakframe_core::item::{ItemState, PublishBasis, ReviewMode};
use cloakframe_engine::batch::{job_from_scan, session_from_report};
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::fallback::{Acceleration, Backends, ModelBackend};
use cloakframe_engine::detect::models::{DetectionModels, FaceModel};
use cloakframe_engine::detect::runtime::OrtRuntime;
use cloakframe_engine::detect::{Detector, DetectorHandle, NoDetections};
use cloakframe_engine::photo::metadata::MetadataPolicy;
use cloakframe_engine::photo::process::{PhotoProcessor, PhotoSettings};
use cloakframe_engine::publish::remove_stale_temp_files;
use cloakframe_engine::review::{ReviewDecisions, apply};
use cloakframe_engine::run::{ItemProcessor, Job, RunReport, start};
use cloakframe_engine::scan::{ScanRequest, scan};
use cloakframe_engine::session::{SessionId, SessionStore};

const USAGE: &str = "\
usage:
  cloakframe-cli run --output DIR --ort-library FILE --models DIR
                     [--faces yolo5face-n|yunet|off] [--plates on|off] [--gpu on|off]
                     [--review required|off] [--keep-metadata [--keep-gps]] [--recursive]
                     [--workers N] [--sessions DIR] INPUT...
  cloakframe-cli review --session ID --decisions FILE [--faces yolo5face-n|yunet|off]
                        [--plates on|off] [--keep-metadata [--keep-gps]] [--workers N]
                        [--sessions DIR]
  cloakframe-cli sessions [--sessions DIR]
  cloakframe-cli --version

--ort-library is the ONNX Runtime library to load. --models is the folder that holds the model
files under their published names (yolov5n_face.onnx, face_detection_yunet_2023mar.onnx,
yolo-v9-t-512-license-plates-end2end.onnx); each must be the one this build pins. --faces picks
the face model (default yolo5face-n) and --plates turns license plates on or off (default off).
--gpu off keeps every model on the CPU; by default each runs on the GPU where this build can, and
a model the GPU fails moves to the CPU for the rest of the run. review takes the settings run was given; decisions on items analyzed with other settings are
refused.
Test builds also take --synthetic, and with it --videos, to run without a media pipeline.

exit status: 0 done, 1 failed or refused items or a failed run, 2 usage error, 3 items wait for
review";

const DONE: u8 = 0;
const FAILED: u8 = 1;
const USAGE_ERROR: u8 = 2;
const NEEDS_REVIEW: u8 = 3;

enum CliError {
    /// The command line itself is wrong; the usage text follows the message.
    Usage(String),
    /// Something failed while carrying out a valid command.
    Failed(String),
}

fn usage(message: impl Into<String>) -> CliError {
    CliError::Usage(message.into())
}

fn failed(message: impl Into<String>) -> CliError {
    CliError::Failed(message.into())
}

struct Args {
    rest: std::vec::IntoIter<String>,
    positional: Vec<String>,
}

impl Args {
    fn value(&mut self, flag: &str) -> Result<String, String> {
        self.rest
            .next()
            .ok_or_else(|| format!("{flag} needs a value"))
    }
}

#[derive(Default)]
struct Options {
    output: Option<PathBuf>,
    review: Option<ReviewMode>,
    recursive: bool,
    videos: bool,
    workers: Option<usize>,
    sessions: Option<PathBuf>,
    synthetic: bool,
    session: Option<String>,
    decisions: Option<PathBuf>,
    ort_library: Option<PathBuf>,
    models: Option<PathBuf>,
    faces: Option<Option<FaceModel>>,
    plates: Option<bool>,
    gpu: Option<Acceleration>,
    keep_metadata: bool,
    keep_gps: bool,
    inputs: Vec<PathBuf>,
}

fn parse(args: Vec<String>) -> Result<Options, String> {
    let mut args = Args {
        rest: args.into_iter(),
        positional: Vec::new(),
    };
    let mut o = Options::default();
    while let Some(arg) = args.rest.next() {
        match arg.as_str() {
            "--output" => o.output = Some(args.value(&arg)?.into()),
            "--review" => {
                o.review = Some(match args.value(&arg)?.as_str() {
                    "required" => ReviewMode::Required,
                    "off" => ReviewMode::Off,
                    other => return Err(format!("--review is required or off, not {other}")),
                })
            }
            "--recursive" => o.recursive = true,
            "--videos" => o.videos = true,
            "--workers" => {
                let value = args.value(&arg)?;
                o.workers =
                    Some(value.parse().ok().filter(|&n| n > 0).ok_or_else(|| {
                        format!("--workers needs a positive number, not {value}")
                    })?);
            }
            "--sessions" => o.sessions = Some(args.value(&arg)?.into()),
            "--synthetic" => o.synthetic = true,
            "--session" => o.session = Some(args.value(&arg)?),
            "--decisions" => o.decisions = Some(args.value(&arg)?.into()),
            "--ort-library" => o.ort_library = Some(args.value(&arg)?.into()),
            "--models" => o.models = Some(args.value(&arg)?.into()),
            "--faces" => {
                o.faces = Some(match args.value(&arg)?.as_str() {
                    "yolo5face-n" => Some(FaceModel::Yolo5FaceN),
                    "yunet" => Some(FaceModel::YuNet),
                    "off" => None,
                    other => {
                        return Err(format!("--faces is yolo5face-n, yunet or off, not {other}"));
                    }
                })
            }
            "--plates" => {
                o.plates = Some(match args.value(&arg)?.as_str() {
                    "on" => true,
                    "off" => false,
                    other => return Err(format!("--plates is on or off, not {other}")),
                })
            }
            "--gpu" => {
                o.gpu = Some(match args.value(&arg)?.as_str() {
                    "on" => Acceleration::Gpu,
                    "off" => Acceleration::CpuOnly,
                    other => return Err(format!("--gpu is on or off, not {other}")),
                })
            }
            "--keep-metadata" => o.keep_metadata = true,
            "--keep-gps" => o.keep_gps = true,
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            _ => args.positional.push(arg),
        }
    }
    o.inputs = args.positional.into_iter().map(PathBuf::from).collect();
    Ok(o)
}

fn session_store(o: &Options) -> Result<SessionStore, CliError> {
    if let Some(dir) = &o.sessions {
        return Ok(SessionStore::new(dir));
    }
    cloakframe_engine::app_dirs::data_dir()
        .map(|dir| SessionStore::new(dir.join("sessions")))
        .ok_or_else(|| failed("no app data folder for this user; pass --sessions DIR"))
}

fn detection_models(o: &Options) -> Result<DetectionModels, CliError> {
    let default = DetectionModels::default();
    DetectionModels::new(
        o.faces.unwrap_or(default.face()),
        o.plates.unwrap_or(default.plates()),
    )
    .map_err(|e| usage(format!("{e}: --faces off needs --plates on")))
}

fn photo_processor(o: &Options) -> Result<PhotoProcessor, CliError> {
    // Builds with the synthetic processor never get here with --synthetic.
    if o.synthetic {
        return Err(usage("this build has no synthetic processor"));
    }
    if o.videos {
        return Err(usage("this build cannot process videos yet"));
    }
    if o.keep_gps && !o.keep_metadata {
        return Err(usage("--keep-gps needs --keep-metadata"));
    }
    let metadata = if o.keep_metadata {
        MetadataPolicy::Allowlist { gps: o.keep_gps }
    } else {
        MetadataPolicy::Strip
    };
    let models = detection_models(o)?;
    PhotoProcessor::new(PhotoSettings {
        models,
        thresholds: models.default_thresholds(),
        metadata,
        ..PhotoSettings::default()
    })
    .map_err(|e| failed(e.to_string()))
}

fn spawn(detector: Box<dyn Detector>) -> Result<DetectorHandle, CliError> {
    DetectorHandle::spawn(detector)
        .map_err(|e| failed(format!("cannot start the inference thread: {e}")))
}

/// The inference thread and where each model on it runs.
struct Started {
    handle: DetectorHandle,
    backends: Backends,
}

fn without_models() -> Result<Started, CliError> {
    Ok(Started {
        handle: spawn(Box::new(NoDetections))?,
        backends: Backends::default(),
    })
}

fn model_name(model: ModelId) -> &'static str {
    match model {
        ModelId::Yolo5FaceN => "YOLO5Face-n",
        ModelId::YuNet => "YuNet",
        ModelId::PlateYolov9T => "the license-plate model",
    }
}

/// Loads ONNX Runtime from `library` and the models from their files in `dir`, and says where
/// each runs.
fn detector(
    library: &Path,
    dir: &Path,
    models: DetectionModels,
    acceleration: Acceleration,
) -> Result<Started, CliError> {
    let runtime = OrtRuntime::load(library).map_err(|e| failed(e.to_string()))?;
    #[cfg(target_os = "macos")]
    let runtime = match cloakframe_engine::app_dirs::cache_dir() {
        Some(cache) => runtime.with_coreml_cache(cache.join("coreml")),
        None => runtime,
    };
    let loaded = models
        .load(&runtime, dir, acceleration)
        .map_err(|e| failed(e.to_string()))?;
    for m in loaded.backends.get() {
        match &m.fallback {
            Some(f) => eprintln!(
                "{} runs on the CPU: it could not start on {} ({})",
                model_name(m.model),
                f.from,
                f.reason
            ),
            None => eprintln!("{} runs on {}", model_name(m.model), m.backend),
        }
    }
    Ok(Started {
        handle: spawn(loaded.detector)?,
        backends: loaded.backends,
    })
}

/// Models the GPU failed during the run. The item it failed on is noted in the report.
fn print_fallbacks(backends: &[ModelBackend]) {
    for m in backends {
        if let Some(f) = m.fallback.as_ref().filter(|f| !f.at_start) {
            eprintln!(
                "{} moved to the CPU for the rest of the run: {} failed ({})",
                model_name(m.model),
                f.from,
                f.reason
            );
        }
    }
}

fn state_label(state: ItemState) -> &'static str {
    match state {
        ItemState::Published(PublishBasis::AutomaticUnreviewed) => {
            "published (automatic, unreviewed)"
        }
        ItemState::Published(PublishBasis::Reviewed) => "published (reviewed)",
        ItemState::Published(PublishBasis::ExportedWithWarnings) => "exported with warnings",
        ItemState::Published(PublishBasis::ExportedWithoutAnonymization) => {
            "exported without anonymization"
        }
        ItemState::NeedsReview => "needs review",
        ItemState::Failed => "failed",
        ItemState::Cancelled => "cancelled",
        ItemState::Queued
        | ItemState::Analyzed
        | ItemState::ReadyForRender(_)
        | ItemState::Rendering(_)
        | ItemState::Verified(_) => "unfinished",
    }
}

fn describe(e: &CoverageEvidence) -> String {
    match &e.problem {
        Problem::CappedCandidates { kept, found, .. } => {
            format!("detection limit reached: {kept} of {found} kept")
        }
        Problem::FailedFrame { frame } => format!("frame {frame} could not be analyzed"),
        Problem::UnresolvedGap { track, first, last } => {
            format!("track {track} lost in frames {first}–{last}")
        }
        Problem::WeakCandidate { score, .. } => format!("weak candidate (score {score:.2})"),
    }
}

fn print_report(report: &RunReport) {
    for o in &report.items {
        let mut line = format!(
            "{}\t{}\t{}",
            o.item.0,
            state_label(o.state),
            o.input.display()
        );
        if let Some(failure) = &o.failure {
            line.push_str(&format!("\t{failure}"));
        }
        if o.state == ItemState::NeedsReview {
            for e in o.analysis.evidence.iter().filter(|e| !e.is_resolved()) {
                line.push_str(&format!("\t{}", describe(e)));
            }
        }
        for note in &o.notes {
            line.push_str(&format!("\t{note}"));
        }
        println!("{line}");
    }
}

fn exit_for(report: &RunReport) -> u8 {
    if report.count(|s| matches!(s, ItemState::Failed | ItemState::Cancelled)) > 0 {
        FAILED
    } else if report.count(|s| s == ItemState::NeedsReview) > 0 {
        NEEDS_REVIEW
    } else {
        DONE
    }
}

fn execute<P: ItemProcessor>(
    job: Job,
    processor: Arc<P>,
    detector: DetectorHandle,
) -> Result<RunReport, CliError> {
    let handle = start(job, processor, detector)
        .map_err(|e| failed(format!("cannot start the run: {e}")))?;
    Ok(handle.wait())
}

fn clean_stale(job: &Job) {
    let mut folders: Vec<_> = job
        .items
        .iter()
        .filter_map(|i| i.destination.parent().map(PathBuf::from))
        .collect();
    folders.sort();
    folders.dedup();
    for folder in folders {
        let _ = remove_stale_temp_files(&folder, SystemTime::now());
    }
}

fn run(o: Options) -> Result<u8, CliError> {
    let output = o
        .output
        .clone()
        .ok_or_else(|| usage("run needs --output DIR"))?;
    if o.inputs.is_empty() {
        return Err(usage("run needs at least one input"));
    }
    #[cfg(feature = "synthetic")]
    if o.synthetic {
        let processor = cloakframe_engine::synthetic::SyntheticProcessor;
        return run_with(&o, &output, processor, without_models);
    }
    let processor = photo_processor(&o)?;
    let models = detection_models(&o)?;
    let library = o
        .ort_library
        .clone()
        .ok_or_else(|| usage("run needs --ort-library FILE"))?;
    let dir = o
        .models
        .clone()
        .ok_or_else(|| usage("run needs --models DIR"))?;
    let acceleration = o.gpu.unwrap_or_default();
    run_with(&o, &output, processor, || {
        detector(&library, &dir, models, acceleration)
    })
}

/// The detector is made only once the scan found something to analyze.
fn run_with<P: ItemProcessor>(
    o: &Options,
    output: &Path,
    processor: P,
    detector: impl FnOnce() -> Result<Started, CliError>,
) -> Result<u8, CliError> {
    let store = session_store(o)?;
    let scanned = scan(
        &ScanRequest {
            inputs: o.inputs.clone(),
            recursive: o.recursive,
            include_videos: o.videos,
            output: Some(output.to_path_buf()),
        },
        &CancelToken::new(),
    )
    .map_err(|_| failed("scan cancelled"))?;
    for issue in &scanned.issues {
        eprintln!("cannot read {}: {:?}", issue.path.display(), issue.kind);
    }
    for (extension, count) in &scanned.skipped {
        let shown = if extension.is_empty() {
            "(none)"
        } else {
            extension
        };
        eprintln!("skipped {count} file(s) of unsupported type {shown}");
    }
    let workers = o
        .workers
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    let (job, plan) = job_from_scan(
        &scanned.files,
        output,
        o.review.unwrap_or(ReviewMode::Required),
        workers,
    );
    clean_stale(&job);
    let fingerprint = processor.fingerprint();
    let started = if job.items.is_empty() {
        without_models()?
    } else {
        detector()?
    };
    let report = execute(job, Arc::new(processor), started.handle)?;
    print_report(&report);
    print_fallbacks(&started.backends.get());
    let (session, left_out) = session_from_report(&report, &plan, output, fingerprint);
    for (item, reason) in &left_out {
        println!("{}\tnot kept for review\t{reason}", item.0);
    }
    if let Some(session) = session {
        let id = SessionId::generate();
        store
            .save(&id, &session)
            .map_err(|e| failed(format!("cannot save the review session: {e}")))?;
        println!("session\t{}", id.as_str());
    }
    let mut code = exit_for(&report);
    if !scanned.issues.is_empty() || !left_out.is_empty() {
        code = code.max(FAILED);
    }
    Ok(code)
}

fn review(o: Options) -> Result<u8, CliError> {
    #[cfg(feature = "synthetic")]
    if o.synthetic {
        return review_with(&o, cloakframe_engine::synthetic::SyntheticProcessor);
    }
    let processor = photo_processor(&o)?;
    review_with(&o, processor)
}

fn review_with<P: ItemProcessor>(o: &Options, processor: P) -> Result<u8, CliError> {
    let id = o
        .session
        .as_deref()
        .and_then(SessionId::parse)
        .ok_or_else(|| usage("review needs --session ID (as printed by run)"))?;
    let decisions_path = o
        .decisions
        .clone()
        .ok_or_else(|| usage("review needs --decisions FILE"))?;
    let store = session_store(o)?;
    let mut session = store.load(&id).map_err(|e| failed(e.to_string()))?;
    let decisions: ReviewDecisions = serde_json::from_slice(
        &std::fs::read(&decisions_path)
            .map_err(|e| failed(format!("cannot read the decisions: {e}")))?,
    )
    .map_err(|e| failed(format!("the decisions file is not valid: {e}")))?;

    let applied = apply(&mut session, &decisions, processor.fingerprint());
    for (item, rejection) in &applied.rejected {
        println!("{}\trefused\t{rejection}", item.0);
    }
    let mut code = if applied.rejected.is_empty() {
        DONE
    } else {
        FAILED
    };
    if !applied.jobs.is_empty() {
        let job = Job {
            items: applied.jobs,
            review: ReviewMode::Required,
            workers: o.workers.unwrap_or(1),
        };
        clean_stale(&job);
        // Resumed items are not analyzed again, so no model is loaded.
        let report = execute(job, Arc::new(processor), spawn(Box::new(NoDetections))?)?;
        print_report(&report);
        let finished: Vec<_> = report
            .items
            .iter()
            .filter(|o| matches!(o.state, ItemState::Published(_)))
            .map(|o| o.item)
            .collect();
        session.items.retain(|i| !finished.contains(&i.item));
        code = code.max(exit_for(&report));
    }
    if session.items.is_empty() {
        store
            .remove(&id)
            .map_err(|e| failed(format!("cannot remove the finished session: {e}")))?;
    } else {
        store
            .save(&id, &session)
            .map_err(|e| failed(format!("cannot save the review session: {e}")))?;
        if code == DONE {
            code = NEEDS_REVIEW;
        }
    }
    Ok(code)
}

fn sessions(o: Options) -> Result<u8, CliError> {
    let store = session_store(&o)?;
    for id in store
        .list()
        .map_err(|e| failed(format!("cannot list sessions: {e}")))?
    {
        match store.load(&id) {
            Ok(s) => println!(
                "{}\t{} item(s)\t{}",
                id.as_str(),
                s.items.len(),
                s.output.display()
            ),
            Err(e) => println!("{}\tunreadable\t{e}", id.as_str()),
        }
    }
    Ok(DONE)
}

fn main() -> ExitCode {
    // SAFETY: the first statement of main; no other thread exists yet.
    #[allow(unsafe_code)]
    unsafe {
        cloakframe_engine::detect::runtime::disable_telemetry()
    };
    let mut args = std::env::args().skip(1);
    let command = args.next();
    let rest: Vec<String> = args.collect();
    let result = match command.as_deref() {
        Some("--version") => {
            println!("cloakframe-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(DONE)
        }
        Some(c @ ("run" | "review" | "sessions")) => {
            parse(rest).map_err(usage).and_then(|o| match c {
                "run" => run(o),
                "review" => review(o),
                _ => sessions(o),
            })
        }
        _ => Err(usage("")),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(CliError::Failed(message)) => {
            eprintln!("error: {message}");
            ExitCode::from(FAILED)
        }
        Err(CliError::Usage(message)) => {
            if !message.is_empty() {
                eprintln!("error: {message}");
            }
            eprintln!("{USAGE}");
            ExitCode::from(USAGE_ERROR)
        }
    }
}
