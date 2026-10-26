//! A run started from the window: the engine works on its own threads and reports here through
//! messages, so drawing never waits for it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use cloakframe_core::detection::{KindThresholds, Thresholds};
use cloakframe_core::item::{ItemId, ItemState, ReviewMode};
use cloakframe_core::mask::{MaskStyle, Method as MaskMethod, MosaicStrength};
use cloakframe_engine::batch::{Kept, job_from_scan, keep_for_review, remove_stale_outputs};
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::fallback::Acceleration;
use cloakframe_engine::detect::models::{DetectionModels, FaceModel as EngineFaceModel, LoadError};
use cloakframe_engine::detect::runtime::OrtRuntime;
use cloakframe_engine::events::{Event, ProgressBoard};
use cloakframe_engine::media::encode::{OutputCodec, Quality};
use cloakframe_engine::media_processor::MediaProcessor;
use cloakframe_engine::model_files::{ModelStore, model_file};
use cloakframe_engine::photo::metadata::MetadataPolicy;
use cloakframe_engine::photo::process::{PhotoProcessor, PhotoSettings};
use cloakframe_engine::run::{Failure as ItemFailure, ItemProcessor, Note, RunReport, start_under};
use cloakframe_engine::scan::ScannedFile;
use cloakframe_engine::session::{SessionId, SessionStore};
use cloakframe_engine::startup::{self, StartError};
use cloakframe_engine::video::analysis::AnalysisSettings;
use cloakframe_engine::video::process::{Tools as VideoTools, VideoProcessor, VideoSettings};
use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::progress::{self, Overall, Pace};
use crate::settings::{Advanced, FaceModel, Method, Settings, VideoCodec, VideoQuality};
use crate::tools::Tools;
use crate::wording::RunNote;

/// Where a run keeps its review sessions and finds its models.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Folders {
    pub data: Option<PathBuf>,
    pub models: Option<PathBuf>,
    /// The models folder is the app's own, which takes the models 1.12.1 downloaded.
    pub app_models: bool,
}

impl Folders {
    pub fn of_the_app() -> Self {
        Self {
            data: cloakframe_engine::app_dirs::data_dir(),
            models: cloakframe_engine::app_dirs::model_dir(),
            app_models: true,
        }
    }

    /// Where runs keep the items they leave for review, which the CLI's sessions share.
    pub fn sessions(&self) -> Option<SessionStore> {
        self.data
            .as_ref()
            .map(|data| SessionStore::new(data.join("sessions")))
    }
}

/// Everything a run needs, taken from the window when Start is pressed.
pub struct Request {
    pub files: Vec<ScannedFile>,
    pub output: PathBuf,
    pub settings: Settings,
    pub tools: Tools,
    pub folders: Folders,
}

#[derive(Debug)]
pub enum Message {
    /// The run is going; its items in scan order, their progress, and what cancels each.
    Started {
        items: Vec<(ItemId, PathBuf)>,
        progress: ProgressBoard,
        cancels: BTreeMap<ItemId, CancelToken>,
    },
    /// The work an item the engine has not reached yet will be, from the start of its file.
    Estimated {
        item: ItemId,
        work: u64,
    },
    State {
        item: ItemId,
        state: ItemState,
    },
    /// Something the user should hear of that does not stop the run.
    Note(RunNote),
    Finished(Finished),
    /// The run could not start.
    Failed(Failure),
}

#[derive(Debug)]
pub enum Failure {
    /// No ONNX Runtime library was found.
    NoRuntime,
    /// A model is not in the model folder; it has to be downloaded.
    MissingModel(&'static str),
    NoDataFolder,
    /// The engine's English, as the CLI words it.
    Other(String),
}

#[derive(Debug)]
pub struct Finished {
    pub report: RunReport,
    /// The session holding the items that wait for review, if any do and it was saved.
    pub session: Option<SessionId>,
    pub problems: Vec<RunNote>,
}

pub struct Running {
    messages: mpsc::Receiver<Message>,
    cancel: CancelToken,
}

impl Running {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn messages(&self) -> impl Iterator<Item = Message> + '_ {
        self.messages.try_iter()
    }

    /// Blocks for the next message, for tests.
    pub fn wait(&self) -> Option<Message> {
        self.messages.recv().ok()
    }
}

/// Sends to the window and has it drawn again.
#[derive(Clone)]
struct Tell {
    send: mpsc::Sender<Message>,
    repaint: egui::Context,
}

impl Tell {
    fn tell(&self, message: Message) {
        let _ = self.send.send(message);
        self.repaint.request_repaint();
    }
}

pub fn start(request: Request, ctx: &egui::Context) -> std::io::Result<Running> {
    let (send, messages) = mpsc::channel();
    let cancel = CancelToken::new();
    let token = cancel.clone();
    let tell = Tell {
        send,
        repaint: ctx.clone(),
    };
    std::thread::Builder::new()
        .name("cloakframe-app-run".into())
        .spawn(move || {
            if let Err(failure) = run(request, &token, &tell) {
                tell.tell(Message::Failed(failure));
            }
        })?;
    Ok(Running { messages, cancel })
}

fn other(e: impl std::fmt::Display) -> Failure {
    Failure::Other(e.to_string())
}

/// The settings a run processes with, which its review session keeps so that the review renders
/// with them whatever the main screen says by then.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunOptions {
    pub faces: bool,
    pub plates: bool,
    pub method: Method,
    pub mosaic_strength: u8,
    /// Missing from sessions saved before there were advanced settings, which ran with the
    /// defaults.
    #[serde(default)]
    pub advanced: Advanced,
}

impl RunOptions {
    pub fn of(settings: &Settings) -> Self {
        Self {
            faces: settings.faces,
            plates: settings.plates,
            method: settings.method,
            mosaic_strength: settings.mosaic_strength,
            advanced: settings.advanced,
        }
    }

    pub fn style(&self) -> MaskStyle {
        let mut style = MaskStyle {
            method: match self.method {
                Method::Fill => MaskMethod::Fill,
                Method::Mosaic => MaskMethod::Mosaic,
                Method::Blur => MaskMethod::Blur,
            },
            ..MaskStyle::default()
        };
        if let Ok(strength) = MosaicStrength::new(self.mosaic_strength) {
            style.mosaic_strength = strength;
        }
        style
    }

    fn models(&self) -> Result<DetectionModels, Failure> {
        let face = self.faces.then_some(match self.advanced.face_model {
            FaceModel::Accurate => EngineFaceModel::Yolo5FaceN,
            FaceModel::Fast => EngineFaceModel::YuNet,
        });
        DetectionModels::new(face, self.plates).map_err(other)
    }

    /// The models' thresholds with the sensitivity's score for both kinds.
    fn thresholds(&self, models: &DetectionModels) -> Thresholds {
        let defaults = models.default_thresholds();
        let score = self.advanced.score();
        let with_score = |t: KindThresholds| {
            KindThresholds::new(score, t.nms_iou()).expect("a slider score is a valid threshold")
        };
        Thresholds {
            face: with_score(defaults.face),
            plate: with_score(defaults.plate),
        }
    }

    pub fn acceleration(&self) -> Acceleration {
        if self.advanced.gpu {
            Acceleration::Gpu
        } else {
            Acceleration::CpuOnly
        }
    }
}

/// What processes a run's items, or renders a review's; `records` holds a video's analysis.
pub fn processor(
    options: &RunOptions,
    tools: &Tools,
    records: PathBuf,
) -> Result<MediaProcessor, Failure> {
    let models = options.models()?;
    let advanced = &options.advanced;
    let photo = PhotoProcessor::new(PhotoSettings {
        models,
        thresholds: options.thresholds(&models),
        tiles: advanced.tiles,
        style: options.style(),
        metadata: if advanced.keep_metadata {
            MetadataPolicy::Allowlist {
                gps: advanced.keep_gps,
            }
        } else {
            MetadataPolicy::Strip
        },
        ..PhotoSettings::default()
    })
    .map_err(other)?;
    let video = match (&tools.ffmpeg, &tools.ffprobe) {
        (Some(ffmpeg), Some(ffprobe)) => {
            let photo_settings = photo.settings();
            let settings = VideoSettings {
                analysis: AnalysisSettings {
                    models: photo_settings.models,
                    thresholds: photo_settings.thresholds,
                    tiles: photo_settings.tiles,
                    ..AnalysisSettings::default()
                },
                style: photo_settings.style,
                codec: match advanced.video_codec {
                    VideoCodec::H264 => OutputCodec::H264,
                    VideoCodec::Hevc => OutputCodec::Hevc,
                },
                quality: match advanced.video_quality {
                    VideoQuality::High => Quality::High,
                    VideoQuality::Balanced => Quality::Balanced,
                    VideoQuality::SpaceSaver => Quality::SpaceSaver,
                },
                hardware_encode: advanced.gpu,
                remove_audio: advanced.remove_audio,
                ..VideoSettings::default()
            };
            Some(VideoProcessor::new(
                VideoTools {
                    ffmpeg: ffmpeg.clone(),
                    ffprobe: ffprobe.clone(),
                },
                settings,
                records,
            ))
        }
        _ => None,
    };
    Ok(MediaProcessor::new(photo, video))
}

/// The process's ONNX Runtime, loaded by the first run that needs it and shared by every later
/// one: ORT allows one environment per process. A load that fails is tried again by the next run.
fn runtime(library: &Path) -> Result<OrtRuntime, Failure> {
    static RUNTIME: Mutex<Option<OrtRuntime>> = Mutex::new(None);
    let mut runtime = RUNTIME.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(loaded) = &*runtime {
        return Ok(loaded.clone());
    }
    let loaded = OrtRuntime::load(library).map_err(other)?;
    *runtime = Some(loaded.clone());
    Ok(loaded)
}

/// Estimates each item's work in turn, the order the run takes them, until all have one or the
/// run is over.
fn estimate(
    processor: &MediaProcessor,
    items: &[(ItemId, PathBuf)],
    cancel: &CancelToken,
    tell: &Tell,
) {
    for (item, input) in items {
        if cancel.is_cancelled() {
            return;
        }
        if let Some(work) = processor.estimated_work(input, cancel) {
            tell.tell(Message::Estimated { item: *item, work });
        }
    }
}

fn run(request: Request, cancel: &CancelToken, tell: &Tell) -> Result<(), Failure> {
    let store = request.folders.sessions().ok_or(Failure::NoDataFolder)?;
    if let Ok(left) = store.sweep() {
        for (path, e) in left {
            tell.tell(Message::Note(RunNote::NotSwept {
                path,
                error: e.to_string(),
            }));
        }
    }
    let id = SessionId::generate();
    let _lock = store.lock(&id).map_err(other)?;

    let options = RunOptions::of(&request.settings);
    let models = options.models()?;
    let processor = Arc::new(processor(&options, &request.tools, store.records_dir(&id))?);
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
    let review = if request.settings.review {
        ReviewMode::Required
    } else {
        ReviewMode::Off
    };
    let (job, plan) = job_from_scan(&request.files, &request.output, review, workers);
    remove_stale_outputs(&job);
    let started = if job.items.is_empty() {
        startup::without_models().map_err(other)?
    } else {
        let library = request
            .tools
            .ort_library
            .as_ref()
            .ok_or(Failure::NoRuntime)?;
        let folder = request
            .folders
            .models
            .clone()
            .ok_or(Failure::NoDataFolder)?;
        startup::with_runtime(
            runtime(library)?,
            &ModelStore::new(folder),
            request.folders.app_models,
            models,
            options.acceleration(),
            |note| tell.tell(Message::Note(RunNote::of_start(&note))),
        )
        .map_err(|e| match e {
            StartError::Load(LoadError::Missing { model, .. }) => {
                Failure::MissingModel(model_file(model).name)
            }
            e => other(e),
        })?
    };
    let items: Vec<(ItemId, PathBuf)> = job.items.iter().map(|i| (i.id, i.input.clone())).collect();
    let handle = start_under(job, Arc::clone(&processor), started.handle, cancel).map_err(other)?;
    tell.tell(Message::Started {
        items: items.clone(),
        progress: handle.progress().clone(),
        cancels: handle.item_cancels(),
    });
    let mut report = None;
    let estimating = cancel.child();
    std::thread::scope(|scope| {
        let spawned = std::thread::Builder::new()
            .name("cloakframe-app-estimate".into())
            .spawn_scoped(scope, || estimate(&processor, &items, &estimating, tell));
        // Without estimates the items the engine has not reached count their kind's average.
        if let Err(e) = spawned {
            tell.tell(Message::Note(RunNote::NoEstimate(e.to_string())));
        }
        for event in handle.events().iter() {
            match event {
                Event::ItemState { item, state } => tell.tell(Message::State { item, state }),
                Event::RunFinished(r) => report = Some(r),
            }
        }
        estimating.cancel();
    });
    let report = handle
        .finish()
        .or(report)
        .ok_or_else(|| Failure::Other("the run ended without a report".to_owned()))?;
    let Kept {
        saved,
        cleaned,
        left_out,
    } = keep_for_review(
        &store,
        &id,
        &report,
        &plan,
        &request.output,
        |input| processor.fingerprint(input),
        serde_json::to_value(options).ok(),
    );
    let mut problems = Vec::new();
    let session = match saved {
        Some(Ok(())) => Some(id.clone()),
        Some(Err(e)) => {
            problems.push(RunNote::SessionNotSaved(e.to_string()));
            None
        }
        None => None,
    };
    if let Err(e) = cleaned {
        problems.push(RunNote::RecordsLeft {
            path: store.records_dir(&id),
            error: e.to_string(),
        });
    }
    for (item, why) in left_out {
        problems.push(RunNote::LeftOut { item, why });
    }
    tell.tell(Message::Finished(Finished {
        report,
        session,
        problems,
    }));
    Ok(())
}

/// A line of a run's log, worded when the window draws it.
#[derive(Debug, Clone, PartialEq)]
pub enum Logged {
    State(ItemId, ItemState),
    Note(RunNote),
    ItemNote(ItemId, Note),
    Failed(ItemId, ItemFailure),
}

/// The latest state of each item of a run, as the window shows it.
#[derive(Debug)]
pub struct Board {
    pub started: Instant,
    pub items: Vec<(ItemId, PathBuf)>,
    pub states: BTreeMap<ItemId, ItemState>,
    pub progress: Option<ProgressBoard>,
    pub estimates: BTreeMap<ItemId, u64>,
    cancels: BTreeMap<ItemId, CancelToken>,
    /// Items the user cancelled that the engine has not stopped yet.
    pub cancelling: BTreeSet<ItemId>,
    pub log: Vec<(Duration, Logged)>,
    pub notes: Vec<RunNote>,
    pub finished: Option<Finished>,
    pub failure: Option<Failure>,
    pace: Pace,
    /// The bar never moves back when an estimate turns out too small.
    shown: f32,
}

impl Board {
    pub fn new(started: Instant) -> Self {
        Self {
            started,
            items: Vec::new(),
            states: BTreeMap::new(),
            progress: None,
            estimates: BTreeMap::new(),
            cancels: BTreeMap::new(),
            cancelling: BTreeSet::new(),
            log: Vec::new(),
            notes: Vec::new(),
            finished: None,
            failure: None,
            pace: Pace::new(started),
            shown: 0.0,
        }
    }

    pub fn take(&mut self, message: Message) {
        let at = self.started.elapsed();
        match message {
            Message::Started {
                items,
                progress,
                cancels,
            } => {
                self.items = items;
                self.progress = Some(progress);
                self.cancels = cancels;
            }
            Message::Estimated { item, work } => {
                self.estimates.insert(item, work);
            }
            Message::State { item, state } => {
                self.states.insert(item, state);
                if state.is_terminal() || state == ItemState::NeedsReview {
                    self.cancelling.remove(&item);
                }
                self.log.push((at, Logged::State(item, state)));
            }
            Message::Note(note) => {
                self.log.push((at, Logged::Note(note.clone())));
                self.notes.push(note);
            }
            Message::Finished(finished) => {
                for outcome in &finished.report.items {
                    if let Some(failure) = &outcome.failure {
                        self.log
                            .push((at, Logged::Failed(outcome.item, failure.clone())));
                    }
                    for note in &outcome.notes {
                        self.log
                            .push((at, Logged::ItemNote(outcome.item, note.clone())));
                    }
                }
                for problem in &finished.problems {
                    self.log.push((at, Logged::Note(problem.clone())));
                }
                self.cancelling.clear();
                self.finished = Some(finished);
            }
            Message::Failed(failure) => self.failure = Some(failure),
        }
    }

    pub fn is_over(&self) -> bool {
        self.finished.is_some() || self.failure.is_some()
    }

    pub fn cancel_item(&mut self, item: ItemId) {
        if let Some(token) = self.cancels.get(&item) {
            token.cancel();
            self.cancelling.insert(item);
        }
    }

    pub fn overall(&self) -> Overall {
        let snapshot = self
            .progress
            .as_ref()
            .map(ProgressBoard::snapshot)
            .unwrap_or_default();
        progress::overall(&self.items, &self.states, &snapshot, &self.estimates)
    }

    /// Takes the progress as of `now`: the fraction the bar shows and the time left, if there is
    /// a pace to go on.
    pub fn measure(&mut self, now: Instant) -> (f32, Option<Duration>) {
        let overall = self.overall();
        self.pace.record(now, overall.done);
        if let Some(fraction) = overall.fraction() {
            self.shown = self.shown.max(fraction);
        }
        (self.shown, self.pace.left(now, overall))
    }

    pub fn name(&self, item: ItemId) -> String {
        self.items
            .iter()
            .find(|(id, _)| *id == item)
            .and_then(|(_, path)| path.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cancelling one item cancels its token only, and the row shows it as cancelling until the
    /// engine says the item stopped.
    #[test]
    fn an_item_is_cancelled_alone_until_the_engine_stops_it() {
        let run = CancelToken::new();
        let (a, b) = (run.child(), run.child());
        let mut board = Board::new(Instant::now());
        board.take(Message::Started {
            items: vec![
                (ItemId(0), PathBuf::from("a.jpg")),
                (ItemId(1), PathBuf::from("b.jpg")),
            ],
            progress: ProgressBoard::default(),
            cancels: BTreeMap::from([(ItemId(0), a.clone()), (ItemId(1), b.clone())]),
        });
        board.cancel_item(ItemId(1));
        assert!(b.is_cancelled() && !a.is_cancelled() && !run.is_cancelled());
        assert!(board.cancelling.contains(&ItemId(1)));
        board.take(Message::State {
            item: ItemId(1),
            state: ItemState::Analyzed,
        });
        assert!(board.cancelling.contains(&ItemId(1)));
        board.take(Message::State {
            item: ItemId(1),
            state: ItemState::Cancelled,
        });
        assert!(board.cancelling.is_empty());
        assert_eq!(
            board.log.last().map(|(_, l)| l),
            Some(&Logged::State(ItemId(1), ItemState::Cancelled))
        );
        assert_eq!(board.name(ItemId(1)), "b.jpg");
    }

    /// Every advanced setting reaches the engine: each changes what the photo or the video
    /// processor fingerprints, so a review made with other settings is refused rather than
    /// rendered with the main screen's.
    #[test]
    fn every_advanced_setting_changes_what_the_engine_processes_with() {
        let tools = Tools {
            ffmpeg: Some(PathBuf::from("ffmpeg")),
            ffprobe: Some(PathBuf::from("ffprobe")),
            ..Tools::default()
        };
        let fingerprints = |advanced: Advanced| {
            let options = RunOptions {
                advanced,
                ..RunOptions::of(&Settings::default())
            };
            let processor = processor(&options, &tools, PathBuf::from("records")).unwrap();
            (
                processor.fingerprint(Path::new("a.jpg")),
                processor.fingerprint(Path::new("a.mp4")),
            )
        };
        let default = Advanced::default();
        let (photo, video) = fingerprints(default);
        let changed: [(&str, Advanced, bool, bool); 8] = [
            (
                "model",
                Advanced {
                    face_model: FaceModel::Fast,
                    ..default
                },
                true,
                true,
            ),
            (
                "sensitivity",
                Advanced {
                    sensitivity: 5,
                    ..default
                },
                true,
                true,
            ),
            (
                "tiles",
                Advanced {
                    tiles: false,
                    ..default
                },
                true,
                true,
            ),
            (
                "metadata",
                Advanced {
                    keep_metadata: true,
                    ..default
                },
                true,
                false,
            ),
            (
                "audio",
                Advanced {
                    remove_audio: true,
                    ..default
                },
                false,
                true,
            ),
            (
                "quality",
                Advanced {
                    video_quality: VideoQuality::SpaceSaver,
                    ..default
                },
                false,
                true,
            ),
            (
                "codec",
                Advanced {
                    video_codec: VideoCodec::Hevc,
                    ..default
                },
                false,
                true,
            ),
            (
                "gpu",
                Advanced {
                    gpu: false,
                    ..default
                },
                false,
                true,
            ),
        ];
        for (name, advanced, photo_changes, video_changes) in changed {
            let (p, v) = fingerprints(advanced);
            assert_eq!(p != photo, photo_changes, "{name} and the photo");
            assert_eq!(v != video, video_changes, "{name} and the video");
        }
        let gps = Advanced {
            keep_metadata: true,
            ..default
        };
        assert_ne!(
            fingerprints(gps).0,
            fingerprints(Advanced {
                keep_gps: true,
                ..gps
            })
            .0
        );
        assert_eq!(
            RunOptions {
                advanced: Advanced {
                    gpu: false,
                    ..default
                },
                ..RunOptions::of(&Settings::default())
            }
            .acceleration(),
            Acceleration::CpuOnly
        );
    }

    /// A session keeps its run's advanced settings, and one saved before there were any reads
    /// as the defaults those runs used.
    #[test]
    fn sessions_keep_the_advanced_settings() {
        let options = RunOptions {
            advanced: Advanced {
                face_model: FaceModel::Fast,
                sensitivity: 2,
                remove_audio: true,
                ..Advanced::default()
            },
            ..RunOptions::of(&Settings::default())
        };
        let stored = serde_json::to_value(options).unwrap();
        assert_eq!(
            serde_json::from_value::<RunOptions>(stored).unwrap(),
            options
        );
        let before = serde_json::json!({
            "faces": true, "plates": false, "method": "mosaic", "mosaic_strength": 8
        });
        assert_eq!(
            serde_json::from_value::<RunOptions>(before)
                .unwrap()
                .advanced,
            Advanced::default()
        );
    }
}
