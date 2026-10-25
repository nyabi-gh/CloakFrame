use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use cloakframe_core::coverage::{CoverageEvidence, Problem};
use cloakframe_core::detection::{DetectionKind, ModelId};
use cloakframe_core::geometry::Rect;
use cloakframe_core::item::{ItemState, PublishBasis, ReviewMode};
use cloakframe_engine::batch::{Kept, job_from_scan, keep_for_review, remove_stale_outputs};
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::fallback::{Acceleration, ModelBackend};
use cloakframe_engine::detect::models::{DetectionModels, FaceModel, LoadError};
use cloakframe_engine::detect::{Detector, DetectorHandle, NoDetections};
use cloakframe_engine::media::encode::{OutputCodec, Quality};
use cloakframe_engine::media_processor::MediaProcessor;
use cloakframe_engine::model_files::download::{Https, download};
use cloakframe_engine::model_files::{MODEL_FILES, ModelStore, Status, model_file};
use cloakframe_engine::photo::metadata::MetadataPolicy;
use cloakframe_engine::photo::process::{PhotoProcessor, PhotoSettings};
use cloakframe_engine::review::{ReviewDecisions, apply};
use cloakframe_engine::run::{ItemProcessor, Job, RunReport, start_under};
use cloakframe_engine::scan::{ScanRequest, scan};
use cloakframe_engine::session::{SessionError, SessionId, SessionLock, SessionStore, StoredMedia};
use cloakframe_engine::startup::{self, StartError, Started};
use cloakframe_engine::video::analysis::AnalysisSettings;
use cloakframe_engine::video::process::{Tools, VideoProcessor, VideoSettings};

const USAGE: &str = "\
usage:
  cloakframe-cli run --output DIR --ort-library FILE [--models DIR]
                     [--faces yolo5face-n|yunet|off] [--plates on|off] [--tiles on|off]
                     [--gpu on|off] [--review required|off] [--keep-metadata [--keep-gps]]
                     [--recursive] [--workers N] [--sessions DIR]
                     [--videos --ffmpeg FILE --ffprobe FILE [--mask X,Y,W,H...]
                      [--video-codec h264|hevc] [--video-quality high|balanced|space-saver]
                      [--hardware-encode on|off]] INPUT...
  cloakframe-cli review --session ID --decisions FILE [--faces yolo5face-n|yunet|off]
                        [--plates on|off] [--tiles on|off] [--keep-metadata [--keep-gps]]
                        [--workers N] [--sessions DIR] [--videos --ffmpeg FILE --ffprobe FILE
                        [--mask X,Y,W,H...] [--video-codec h264|hevc]
                        [--video-quality high|balanced|space-saver] [--hardware-encode on|off]]
  cloakframe-cli models [--models DIR]
  cloakframe-cli models fetch [--accept-terms] [--models DIR] yolo5face-n|yunet|plates...
  cloakframe-cli sessions [--sessions DIR]
  cloakframe-cli --version

--ort-library is the ONNX Runtime library to load. --models is the folder that holds the model
files under their published names; each must be the one this build pins. By default it is the
app's model folder, into which models CloakFrame 1.12.1 downloaded are copied. models lists the
models with their terms, and models fetch downloads them from their projects on GitHub (nothing
is uploaded). YOLO5Face-n is for non-commercial use only; --accept-terms accepts that.
--faces picks the face model (default yolo5face-n) and --plates turns license plates on or off
(default off). --tiles off detects each photo in one whole view only; by default overlapping
tiles are detected as well, which finds small faces in large photos at more cost. --gpu off keeps every model on the CPU; by default each runs on the GPU where this
build can, and a model the GPU fails moves to the CPU for the rest of the run. review takes the
settings run was given, the video ones too; decisions on items analyzed with other settings are
refused. A decision may carry its item's key from the session (the revision's key); one made
for another input, models, settings or timeline is refused.
--videos processes MP4, MOV, M4V and WebM files as well, with the given FFmpeg and ffprobe.
Every frame of a video is detected with the same models and settings as photos, and what is
tracked is masked; each --mask area (display pixels, repeatable) is masked in every frame as
well. A video waiting for review keeps its frame-by-frame records in the session folder until
it is published or the session is done; the session lists its tracks, and a decision can leave
tracks unmasked (excluded_tracks) or add tracks drawn by hand (added_tracks). review renders a
video again only from the same bytes and timeline it analyzed. Videos are written as MP4, H.264
by default at high quality; --hardware-encode off skips the hardware encoders, and one that
fails during the run leaves the item and the rest of the run to software.
Test builds also take --synthetic, and with it --videos, to run without a media pipeline, and
--fixed-detection X,Y,W,H in place of --ort-library, which finds that face (in the pixels of
the image the models are given) in every image; use it with --tiles off.

Ctrl+C cancels: items in progress stop and nothing more is published. A second Ctrl+C exits at
once. Closing the terminal only cancels.

exit status: 0 done, 1 failed or refused items or a failed run, 2 usage error, 3 items wait for
review, 130 cancelled";

const DONE: u8 = 0;
const FAILED: u8 = 1;
const USAGE_ERROR: u8 = 2;
const NEEDS_REVIEW: u8 = 3;
const CANCELLED: u8 = 130;

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
    tiles: Option<bool>,
    gpu: Option<Acceleration>,
    keep_metadata: bool,
    keep_gps: bool,
    accept_terms: bool,
    ffmpeg: Option<PathBuf>,
    ffprobe: Option<PathBuf>,
    masks: Vec<Rect>,
    fixed_detection: Option<Rect>,
    video_codec: Option<OutputCodec>,
    video_quality: Option<Quality>,
    hardware_encode: Option<bool>,
    inputs: Vec<PathBuf>,
}

fn parse_rect(flag: &str, value: &str) -> Result<Rect, String> {
    let fields: Vec<f32> = value
        .split(',')
        .map(|v| v.trim().parse::<f32>())
        .collect::<Result<_, _>>()
        .map_err(|_| format!("{flag} is X,Y,W,H in pixels, not {value}"))?;
    let [x, y, width, height] = fields[..] else {
        return Err(format!("{flag} is X,Y,W,H in pixels, not {value}"));
    };
    Rect::new(x, y, width, height).map_err(|e| format!("{flag} {value}: {e}"))
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
            "--tiles" => {
                o.tiles = Some(match args.value(&arg)?.as_str() {
                    "on" => true,
                    "off" => false,
                    other => return Err(format!("--tiles is on or off, not {other}")),
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
            "--accept-terms" => o.accept_terms = true,
            "--ffmpeg" => o.ffmpeg = Some(args.value(&arg)?.into()),
            "--ffprobe" => o.ffprobe = Some(args.value(&arg)?.into()),
            "--mask" => o.masks.push(parse_rect(&arg, &args.value(&arg)?)?),
            "--fixed-detection" => {
                o.fixed_detection = Some(parse_rect(&arg, &args.value(&arg)?)?);
            }
            "--video-codec" => {
                o.video_codec = Some(match args.value(&arg)?.as_str() {
                    "h264" => OutputCodec::H264,
                    "hevc" => OutputCodec::Hevc,
                    other => return Err(format!("--video-codec is h264 or hevc, not {other}")),
                })
            }
            "--video-quality" => {
                o.video_quality = Some(match args.value(&arg)?.as_str() {
                    "high" => Quality::High,
                    "balanced" => Quality::Balanced,
                    "space-saver" => Quality::SpaceSaver,
                    other => {
                        return Err(format!(
                            "--video-quality is high, balanced or space-saver, not {other}"
                        ));
                    }
                })
            }
            "--hardware-encode" => {
                o.hardware_encode = Some(match args.value(&arg)?.as_str() {
                    "on" => true,
                    "off" => false,
                    other => return Err(format!("--hardware-encode is on or off, not {other}")),
                })
            }
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
        tiles: o.tiles.unwrap_or(PhotoSettings::default().tiles),
        metadata,
        ..PhotoSettings::default()
    })
    .map_err(|e| failed(e.to_string()))
}

/// Photos, and with --videos videos masked with the given areas, their records in `records`.
fn media_processor(o: &Options, records: PathBuf) -> Result<MediaProcessor, CliError> {
    let photo = photo_processor(o)?;
    let video_flags = o.ffmpeg.is_some()
        || o.ffprobe.is_some()
        || !o.masks.is_empty()
        || o.video_codec.is_some()
        || o.video_quality.is_some()
        || o.hardware_encode.is_some();
    if !o.videos {
        if video_flags {
            return Err(usage("the video options need --videos"));
        }
        return Ok(MediaProcessor::new(photo, None));
    }
    let tools = Tools {
        ffmpeg: o
            .ffmpeg
            .clone()
            .ok_or_else(|| usage("--videos needs --ffmpeg FILE"))?,
        ffprobe: o
            .ffprobe
            .clone()
            .ok_or_else(|| usage("--videos needs --ffprobe FILE"))?,
    };
    let default = VideoSettings::default();
    let photo_settings = photo.settings();
    let settings = VideoSettings {
        codec: o.video_codec.unwrap_or(default.codec),
        quality: o.video_quality.unwrap_or(default.quality),
        hardware_encode: o.hardware_encode.unwrap_or(default.hardware_encode),
        masks: o.masks.clone(),
        analysis: AnalysisSettings {
            models: photo_settings.models,
            thresholds: photo_settings.thresholds,
            tiles: photo_settings.tiles,
            ..AnalysisSettings::default()
        },
        ..default
    };
    Ok(MediaProcessor::new(
        photo,
        Some(VideoProcessor::new(tools, settings, records)),
    ))
}

fn spawn(detector: Box<dyn Detector>) -> Result<DetectorHandle, CliError> {
    startup::spawn(detector).map_err(|e| failed(e.to_string()))
}

fn without_models() -> Result<Started, CliError> {
    startup::without_models().map_err(|e| failed(e.to_string()))
}

fn model_name(model: ModelId) -> &'static str {
    match model {
        ModelId::Yolo5FaceN => "YOLO5Face-n",
        ModelId::YuNet => "YuNet",
        ModelId::PlateYolov9T => "the license-plate model",
    }
}

/// The name `models fetch` takes.
fn fetch_name(model: ModelId) -> &'static str {
    match model {
        ModelId::Yolo5FaceN => "yolo5face-n",
        ModelId::YuNet => "yunet",
        ModelId::PlateYolov9T => "plates",
    }
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

/// The model folder: --models, or the app's (`true`).
fn model_store(o: &Options) -> Result<(ModelStore, bool), CliError> {
    if let Some(dir) = &o.models {
        return Ok((ModelStore::new(dir), false));
    }
    cloakframe_engine::app_dirs::model_dir()
        .map(|dir| (ModelStore::new(dir), true))
        .ok_or_else(|| failed("no app data folder for this user; pass --models DIR"))
}

/// Loads ONNX Runtime from `library` and the models from `store`, and says where each runs.
fn detector(
    library: &Path,
    (store, app_folder): &(ModelStore, bool),
    models: DetectionModels,
    acceleration: Acceleration,
) -> Result<Started, CliError> {
    let started = startup::with_models(library, store, *app_folder, models, acceleration, |n| {
        eprintln!("{n}")
    })
    .map_err(|e| match e {
        StartError::Load(e @ LoadError::Missing { model, .. }) => failed(format!(
            "{e}; download it with `cloakframe-cli models fetch {}{}`",
            fetch_name(model),
            if *app_folder { "" } else { " --models DIR" }
        )),
        e => failed(e.to_string()),
    })?;
    for m in started.backends.get() {
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
    Ok(started)
}

/// Models the GPU failed during the run. The item it failed on is noted in the report.
fn print_fallbacks(backends: &[ModelBackend]) {
    for m in backends {
        if let Some(f) = m.fallback.as_ref().filter(|f| !f.at_start) {
            note(format_args!(
                "{} moved to the CPU for the rest of the run: {} failed ({})",
                model_name(m.model),
                f.from,
                f.reason
            ));
        }
    }
}

/// A line of the results. Once the terminal is gone (a hangup) or the reader stops reading, the
/// line has nowhere to go; `println!` would panic there, before the exit code is returned, so a
/// failed write is dropped instead.
fn out(line: impl fmt::Display) {
    let _ = writeln!(io::stdout().lock(), "{line}");
}

/// A message on stderr, dropped like [`out`] when stderr is gone.
fn note(line: impl fmt::Display) {
    let _ = writeln!(io::stderr().lock(), "{line}");
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
        Problem::UnresolvedGap {
            detection,
            first,
            last,
            ..
        } => {
            let subject = match detection {
                DetectionKind::Face => "face",
                DetectionKind::Plate => "license plate",
            };
            format!("{subject} lost in frames {first}–{last}")
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
        out(line);
    }
}

/// The code for two outcomes at once: a failure outweighs items waiting for review.
fn worse(a: u8, b: u8) -> u8 {
    let rank = |code| match code {
        FAILED => 2,
        NEEDS_REVIEW => 1,
        _ => 0,
    };
    if rank(b) > rank(a) { b } else { a }
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
    cancel: &CancelToken,
) -> Result<RunReport, CliError> {
    let handle = start_under(job, processor, detector, cancel)
        .map_err(|e| failed(format!("cannot start the run: {e}")))?;
    Ok(handle.wait())
}

fn run(o: Options, cancel: &CancelToken) -> Result<u8, CliError> {
    let output = o
        .output
        .clone()
        .ok_or_else(|| usage("run needs --output DIR"))?;
    if o.inputs.is_empty() {
        return Err(usage("run needs at least one input"));
    }
    let store = session_store(&o)?;
    let session = SessionId::generate();
    #[cfg(feature = "synthetic")]
    if o.synthetic {
        let processor = cloakframe_engine::synthetic::SyntheticProcessor;
        return run_with(
            &o,
            &output,
            processor,
            without_models,
            (store, session),
            cancel,
        );
    }
    let processor = media_processor(&o, store.records_dir(&session))?;
    #[cfg(feature = "synthetic")]
    if let Some(found) = o.fixed_detection {
        let detector = || {
            Ok(Started {
                handle: spawn(Box::new(cloakframe_engine::synthetic::FixedDetection(
                    found,
                )))?,
                backends: cloakframe_engine::detect::fallback::Backends::default(),
            })
        };
        return run_with(&o, &output, processor, detector, (store, session), cancel);
    }
    #[cfg(not(feature = "synthetic"))]
    if o.fixed_detection.is_some() {
        return Err(usage("this build has no fixed detection"));
    }
    let models = detection_models(&o)?;
    let library = o
        .ort_library
        .clone()
        .ok_or_else(|| usage("run needs --ort-library FILE"))?;
    let models_store = model_store(&o)?;
    let acceleration = o.gpu.unwrap_or_default();
    run_with(
        &o,
        &output,
        processor,
        || detector(&library, &models_store, models, acceleration),
        (store, session),
        cancel,
    )
}

/// The detector is made only once the scan found something to analyze. The session's id is
/// chosen up front, as its records folder holds the videos' analysis during the run.
fn run_with<P: ItemProcessor>(
    o: &Options,
    output: &Path,
    processor: P,
    detector: impl FnOnce() -> Result<Started, CliError>,
    (store, id): (SessionStore, SessionId),
    cancel: &CancelToken,
) -> Result<u8, CliError> {
    let _lock = hold(&store, &id)?;
    let scanned = scan(
        &ScanRequest {
            inputs: o.inputs.clone(),
            recursive: o.recursive,
            include_videos: o.videos,
            output: Some(output.to_path_buf()),
        },
        cancel,
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
    remove_stale_outputs(&job);
    let started = if job.items.is_empty() {
        without_models()?
    } else {
        detector()?
    };
    let processor = Arc::new(processor);
    let report = execute(job, Arc::clone(&processor), started.handle, cancel)?;
    let Kept {
        saved,
        cleaned,
        left_out,
    } = keep_for_review(
        &store,
        &id,
        &report,
        &plan,
        output,
        |input| processor.fingerprint(input),
        None,
    );
    print_report(&report);
    print_fallbacks(&started.backends.get());
    for (item, reason) in &left_out {
        out(format_args!("{}\tnot kept for review\t{reason}", item.0));
    }
    let mut code = exit_for(&report);
    if let Err(e) = cleaned {
        note(format_args!(
            "cannot remove the video analysis records in {}: {e}",
            store.records_dir(&id).display()
        ));
        code = worse(code, FAILED);
    }
    if let Some(saved) = saved {
        saved.map_err(|e| failed(format!("cannot save the review session: {e}")))?;
        out(format_args!("session\t{}", id.as_str()));
    }
    if !scanned.issues.is_empty() || !left_out.is_empty() {
        code = worse(code, FAILED);
    }
    Ok(code)
}

/// Removes the records runs that ended abnormally left, then holds session `id` for this
/// process (see [`SessionStore::lock`]).
fn hold(store: &SessionStore, id: &SessionId) -> Result<SessionLock, CliError> {
    match store.sweep() {
        Ok(left) => {
            for (path, e) in left {
                note(format_args!(
                    "cannot remove the video analysis records left in {}: {e}",
                    path.display()
                ));
            }
        }
        Err(e) => note(format_args!(
            "cannot look for video analysis records left in {}: {e}",
            store.dir().display()
        )),
    }
    store.lock(id).map_err(|e| match e {
        SessionError::InUse => failed(e.to_string()),
        e => failed(format!(
            "cannot lock the review session in {}: {e}",
            store.dir().display()
        )),
    })
}

fn review(o: Options, cancel: &CancelToken) -> Result<u8, CliError> {
    let id = o
        .session
        .as_deref()
        .and_then(SessionId::parse)
        .ok_or_else(|| usage("review needs --session ID (as printed by run)"))?;
    let store = session_store(&o)?;
    #[cfg(feature = "synthetic")]
    if o.synthetic {
        let processor = cloakframe_engine::synthetic::SyntheticProcessor;
        return review_with(&o, processor, (store, id), cancel);
    }
    let processor = media_processor(&o, store.records_dir(&id))?;
    review_with(&o, processor, (store, id), cancel)
}

fn review_with<P: ItemProcessor>(
    o: &Options,
    processor: P,
    (store, id): (SessionStore, SessionId),
    cancel: &CancelToken,
) -> Result<u8, CliError> {
    let decisions_path = o
        .decisions
        .clone()
        .ok_or_else(|| usage("review needs --decisions FILE"))?;
    let _lock = hold(&store, &id)?;
    let mut session = store.load(&id).map_err(|e| failed(e.to_string()))?;
    let has_videos = session
        .items
        .iter()
        .any(|i| matches!(i.media, StoredMedia::Video { .. }));
    if has_videos && !o.videos {
        return Err(usage(
            "the session holds videos; review them with --videos --ffmpeg FILE --ffprobe FILE and the video options run was given",
        ));
    }
    let decisions: ReviewDecisions = serde_json::from_slice(
        &std::fs::read(&decisions_path)
            .map_err(|e| failed(format!("cannot read the decisions: {e}")))?,
    )
    .map_err(|e| failed(format!("the decisions file is not valid: {e}")))?;

    let applied = apply(&mut session, &decisions, |input| {
        processor.fingerprint(input)
    });
    for (item, rejection) in &applied.rejected {
        out(format_args!("{}\trefused\t{rejection}", item.0));
    }
    let mut code = if applied.rejected.is_empty() {
        DONE
    } else {
        FAILED
    };
    let mut resumed = None;
    if !applied.jobs.is_empty() {
        let job = Job {
            items: applied.jobs,
            review: ReviewMode::Required,
            workers: o.workers.unwrap_or(1),
        };
        remove_stale_outputs(&job);
        // Resumed items are not analyzed again, so no model is loaded.
        let report = execute(
            job,
            Arc::new(processor),
            spawn(Box::new(NoDetections))?,
            cancel,
        )?;
        let finished: Vec<_> = report
            .items
            .iter()
            .filter(|o| matches!(o.state, ItemState::Published(_)))
            .map(|o| o.item)
            .collect();
        session.items.retain(|i| !finished.contains(&i.item));
        code = worse(code, exit_for(&report));
        resumed = Some(report);
    }
    // Stored before the report is written, which can fail once the terminal is gone. The
    // records of the published videos go with them.
    let stored = if session.items.is_empty() {
        store
            .remove(&id)
            .map_err(|e| failed(format!("cannot remove the finished session: {e}")))
    } else {
        if code == DONE {
            code = NEEDS_REVIEW;
        }
        store
            .save(&id, &session)
            .map_err(|e| failed(format!("cannot save the review session: {e}")))
            .and_then(|()| {
                store.prune(&id, &session).map_err(|e| {
                    failed(format!(
                        "cannot remove the video analysis records in {}: {e}",
                        store.records_dir(&id).display()
                    ))
                })
            })
    };
    if let Some(report) = &resumed {
        print_report(report);
    }
    stored?;
    Ok(code)
}

fn models(o: Options, cancel: &CancelToken) -> Result<u8, CliError> {
    let words: Vec<String> = o
        .inputs
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let (store, app_folder) = model_store(&o)?;
    match words.split_first() {
        None => list_models(&store, app_folder),
        Some((command, names)) if command == "fetch" => {
            fetch_models(&store, app_folder, names, o.accept_terms, cancel)
        }
        Some((other, _)) => Err(usage(format!("models takes fetch or nothing, not {other}"))),
    }
}

fn list_models(store: &ModelStore, app_folder: bool) -> Result<u8, CliError> {
    if app_folder {
        startup::import_legacy(store, MODEL_FILES.iter().map(|f| f.model), &mut |n| {
            eprintln!("{n}")
        })
        .map_err(|e| failed(e.to_string()))?;
    }
    for file in &MODEL_FILES {
        let status = match store.status(file.model) {
            Ok(Status::Present) => "present",
            Ok(Status::Missing) => "missing",
            Ok(Status::Different) => "not the pinned file",
            Err(e) => {
                return Err(failed(format!(
                    "cannot read {}: {e}",
                    store.path(file.model).display()
                )));
            }
        };
        println!(
            "{}\t{status}\t{}\t{}",
            fetch_name(file.model),
            megabytes(file.size),
            file.terms.license
        );
        println!("\t{}", file.terms.summary);
        println!("\t{}", file.terms.source);
    }
    println!("folder\t{}", store.dir().display());
    Ok(DONE)
}

fn fetch_models(
    store: &ModelStore,
    app_folder: bool,
    names: &[String],
    accept_terms: bool,
    cancel: &CancelToken,
) -> Result<u8, CliError> {
    if names.is_empty() {
        return Err(usage("models fetch needs yolo5face-n, yunet or plates"));
    }
    let mut wanted = Vec::new();
    for name in names {
        let model = MODEL_FILES
            .iter()
            .map(|f| f.model)
            .find(|&m| fetch_name(m) == name)
            .ok_or_else(|| {
                usage(format!(
                    "models fetch takes yolo5face-n, yunet or plates, not {name}"
                ))
            })?;
        if !wanted.contains(&model) {
            wanted.push(model);
        }
    }
    if app_folder {
        startup::import_legacy(store, wanted.iter().copied(), &mut |n| eprintln!("{n}"))
            .map_err(|e| failed(e.to_string()))?;
    }
    let mut needed = Vec::new();
    for model in wanted {
        match store.status(model) {
            Ok(Status::Present) => {
                eprintln!(
                    "{} is already in {}",
                    model_name(model),
                    store.dir().display()
                );
            }
            Ok(Status::Missing | Status::Different) => needed.push(model),
            Err(e) => {
                return Err(failed(format!(
                    "cannot read {}: {e}",
                    store.path(model).display()
                )));
            }
        }
    }
    let restricted: Vec<_> = needed
        .iter()
        .map(|&m| model_file(m))
        .filter(|f| f.terms.non_commercial_only)
        .collect();
    if !accept_terms && !restricted.is_empty() {
        for file in restricted {
            eprintln!("{}: {}", model_name(file.model), file.terms.summary);
        }
        return Err(usage(
            "pass --accept-terms to download a model whose terms limit its use",
        ));
    }
    let https = Https::default();
    for model in needed {
        let file = model_file(model);
        eprintln!(
            "downloading {} ({}): {}",
            model_name(model),
            megabytes(file.size),
            file.terms.summary
        );
        download(store, model, &https, cancel, &mut |_| {})
            .map_err(|e| failed(format!("cannot download {}: {e}", model_name(model))))?;
        println!("{}\t{}", fetch_name(model), store.path(model).display());
    }
    Ok(DONE)
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
    let cancel = cancel_on_interrupt();
    let mut args = std::env::args().skip(1);
    let command = args.next();
    let rest: Vec<String> = args.collect();
    let result = match command.as_deref() {
        Some("--version") => {
            println!("cloakframe-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(DONE)
        }
        Some(c @ ("run" | "review" | "models" | "sessions")) => {
            parse(rest).map_err(usage).and_then(|o| match c {
                "run" => run(o, &cancel),
                "review" => review(o, &cancel),
                "models" => models(o, &cancel),
                _ => sessions(o),
            })
        }
        _ => Err(usage("")),
    };
    let code = report(result);
    if cancel.is_cancelled() {
        return ExitCode::from(CANCELLED);
    }
    code
}

/// The first Ctrl+C (or SIGTERM) cancels; work in progress stops, temp outputs are removed and
/// nothing more is published. A second one exits at once. A hangup only ever cancels: closing a
/// terminal under zsh sends two, the kernel's and zsh's own.
fn cancel_on_interrupt() -> CancelToken {
    let cancel = CancelToken::new();
    #[cfg(unix)]
    let ignored = ignored_signals();
    let handler = cancel.clone();
    let mut interrupted = false;
    let installed = ctrlc::set_handler(move || {
        if interrupted {
            exit_now(CANCELLED);
        }
        interrupted = true;
        handler.cancel();
        note("cancelling; press Ctrl+C again to stop at once");
    });
    if let Err(e) = installed {
        eprintln!("warning: Ctrl+C will stop without cleaning up: {e}");
    }
    #[cfg(unix)]
    if !ignored.contains(&libc::SIGHUP) {
        cancel_on_hangups(cancel.clone());
    }
    #[cfg(unix)]
    ignore(&ignored);
    cancel
}

/// Ends the process at once, without exit handlers or static destructors. `std::process::exit`
/// runs ONNX Runtime's and CoreML's destructors while the inference thread is still working, which
/// now and then aborted the process ("pure virtual function called") instead of exiting.
#[allow(unsafe_code)]
fn exit_now(code: u8) -> ! {
    #[cfg(unix)]
    {
        // SAFETY: _exit ends the process; nothing after it runs.
        unsafe { libc::_exit(code.into()) }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
        // SAFETY: the pseudo handle of the current process is always valid.
        unsafe { TerminateProcess(GetCurrentProcess(), code.into()) };
        unreachable!("terminating this process does not return")
    }
}

/// The signals this process started with ignored, as `nohup` does with SIGHUP and a script's
/// background job with SIGINT. ctrlc installs its handler over them, so they are ignored again
/// after it.
#[cfg(unix)]
#[allow(unsafe_code)]
fn ignored_signals() -> Vec<libc::c_int> {
    [libc::SIGINT, libc::SIGTERM, libc::SIGHUP]
        .into_iter()
        .filter(|&signal| {
            let mut current = std::mem::MaybeUninit::<libc::sigaction>::zeroed();
            // SAFETY: with no new action, sigaction only writes the current one into `current`,
            // which it then holds whole.
            unsafe {
                libc::sigaction(signal, std::ptr::null(), current.as_mut_ptr()) == 0
                    && current.assume_init().sa_sigaction == libc::SIG_IGN
            }
        })
        .collect()
}

/// What a hangup cancels, set before the handler is installed.
#[cfg(unix)]
static HUNG_UP: std::sync::OnceLock<CancelToken> = std::sync::OnceLock::new();

/// Replaces ctrlc's SIGHUP handler, which cannot tell a hangup from Ctrl+C.
#[cfg(unix)]
#[allow(unsafe_code)]
fn cancel_on_hangups(cancel: CancelToken) {
    extern "C" fn on_hangup(_: libc::c_int) {
        // Only an atomic load and store, and write(2): all a signal handler may do.
        if let Some(cancel) = HUNG_UP.get() {
            cancel.cancel();
        }
        let line = b"hung up; cancelling\n";
        // SAFETY: write(2) is async-signal-safe and reads `line` only.
        unsafe { libc::write(libc::STDERR_FILENO, line.as_ptr().cast(), line.len()) };
    }
    let _ = HUNG_UP.set(cancel);
    // SAFETY: an all-zero sigaction is a valid value to fill in; the handler above is
    // async-signal-safe.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = on_hangup as extern "C" fn(libc::c_int) as libc::sighandler_t;
        action.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(libc::SIGHUP, &action, std::ptr::null_mut());
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn ignore(signals: &[libc::c_int]) {
    for &signal in signals {
        // SAFETY: ignoring is a valid disposition for these signals, and no handler is replaced
        // that anything else still relies on.
        unsafe { libc::signal(signal, libc::SIG_IGN) };
    }
}

fn report(result: Result<u8, CliError>) -> ExitCode {
    match result {
        Ok(code) => ExitCode::from(code),
        Err(CliError::Failed(message)) => {
            note(format_args!("error: {message}"));
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
