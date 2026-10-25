//! A run started from the window: the engine works on its own threads and reports here through
//! messages, so drawing never waits for it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};

use cloakframe_core::item::{ItemId, ItemState, ReviewMode};
use cloakframe_core::mask::{MaskStyle, Method as MaskMethod, MosaicStrength};
use cloakframe_engine::batch::{Kept, job_from_scan, keep_for_review, remove_stale_outputs};
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::fallback::Acceleration;
use cloakframe_engine::detect::models::{DetectionModels, LoadError};
use cloakframe_engine::events::{Event, ProgressBoard};
use cloakframe_engine::media_processor::MediaProcessor;
use cloakframe_engine::model_files::{ModelStore, model_file};
use cloakframe_engine::photo::process::{PhotoProcessor, PhotoSettings};
use cloakframe_engine::run::{ItemProcessor, RunReport, start_under};
use cloakframe_engine::scan::ScannedFile;
use cloakframe_engine::session::{SessionId, SessionStore};
use cloakframe_engine::startup::{self, StartError};
use cloakframe_engine::video::analysis::AnalysisSettings;
use cloakframe_engine::video::process::{Tools as VideoTools, VideoProcessor, VideoSettings};
use eframe::egui;

use crate::settings::{Method, Settings};
use crate::tools::Tools;

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
    /// The run is going; its items in scan order, and their progress.
    Started {
        items: Vec<(ItemId, PathBuf)>,
        progress: ProgressBoard,
    },
    State {
        item: ItemId,
        state: ItemState,
    },
    /// Something the user should hear of that does not stop the run.
    Note(String),
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
    Other(String),
}

#[derive(Debug)]
pub struct Finished {
    pub report: RunReport,
    /// The session holding the items that wait for review, if any do and it was saved.
    pub session: Option<SessionId>,
    pub problems: Vec<String>,
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

pub fn start(request: Request, ctx: &egui::Context) -> std::io::Result<Running> {
    let (send, messages) = mpsc::channel();
    let cancel = CancelToken::new();
    let token = cancel.clone();
    let repaint = ctx.clone();
    std::thread::Builder::new()
        .name("cloakframe-app-run".into())
        .spawn(move || {
            let tell = |message| {
                let _ = send.send(message);
                repaint.request_repaint();
            };
            if let Err(failure) = run(request, &token, &tell) {
                tell(Message::Failed(failure));
            }
        })?;
    Ok(Running { messages, cancel })
}

fn other(e: impl std::fmt::Display) -> Failure {
    Failure::Other(e.to_string())
}

fn detection_models(settings: &Settings) -> Result<DetectionModels, Failure> {
    let face = settings
        .faces
        .then(|| DetectionModels::default().face())
        .flatten();
    DetectionModels::new(face, settings.plates).map_err(other)
}

fn processor(
    request: &Request,
    models: DetectionModels,
    records: PathBuf,
) -> Result<MediaProcessor, Failure> {
    let mut style = MaskStyle {
        method: match request.settings.method {
            Method::Fill => MaskMethod::Fill,
            Method::Mosaic => MaskMethod::Mosaic,
            Method::Blur => MaskMethod::Blur,
        },
        ..MaskStyle::default()
    };
    if let Ok(strength) = MosaicStrength::new(request.settings.mosaic_strength) {
        style.mosaic_strength = strength;
    }
    let photo = PhotoProcessor::new(PhotoSettings {
        models,
        thresholds: models.default_thresholds(),
        style,
        ..PhotoSettings::default()
    })
    .map_err(other)?;
    let video = match (&request.tools.ffmpeg, &request.tools.ffprobe) {
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

fn run(request: Request, cancel: &CancelToken, tell: &impl Fn(Message)) -> Result<(), Failure> {
    let no_folder = || Failure::Other("no app data folder for this user".to_owned());
    let data = request.folders.data.clone().ok_or_else(no_folder)?;
    let store = SessionStore::new(data.join("sessions"));
    if let Ok(left) = store.sweep() {
        for (path, e) in left {
            tell(Message::Note(format!("{}: {e}", path.display())));
        }
    }
    let id = SessionId::generate();
    let _lock = store.lock(&id).map_err(other)?;

    let models = detection_models(&request.settings)?;
    let processor = Arc::new(processor(&request, models, store.records_dir(&id))?);
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
        let folder = request.folders.models.clone().ok_or_else(no_folder)?;
        startup::with_models(
            library,
            &ModelStore::new(folder),
            request.folders.app_models,
            models,
            Acceleration::Gpu,
            |note| tell(Message::Note(note.to_string())),
        )
        .map_err(|e| match e {
            StartError::Load(LoadError::Missing { model, .. }) => {
                Failure::MissingModel(model_file(model).name)
            }
            e => other(e),
        })?
    };
    let items = job.items.iter().map(|i| (i.id, i.input.clone())).collect();
    let handle = start_under(job, Arc::clone(&processor), started.handle, cancel).map_err(other)?;
    tell(Message::Started {
        items,
        progress: handle.progress().clone(),
    });
    let mut report = None;
    for event in handle.events().iter() {
        match event {
            Event::ItemState { item, state } => tell(Message::State { item, state }),
            Event::RunFinished(r) => report = Some(r),
        }
    }
    let report = handle
        .finish()
        .or(report)
        .ok_or_else(|| Failure::Other("the run ended without a report".to_owned()))?;
    let Kept {
        saved,
        cleaned,
        left_out,
    } = keep_for_review(&store, &id, &report, &plan, &request.output, |input| {
        processor.fingerprint(input)
    });
    let mut problems = Vec::new();
    let session = match saved {
        Some(Ok(())) => Some(id.clone()),
        Some(Err(e)) => {
            problems.push(format!("cannot save the review session: {e}"));
            None
        }
        None => None,
    };
    if let Err(e) = cleaned {
        problems.push(format!(
            "cannot remove the video analysis records in {}: {e}",
            store.records_dir(&id).display()
        ));
    }
    for (item, reason) in left_out {
        problems.push(format!("item {}: {reason}", item.0));
    }
    tell(Message::Finished(Finished {
        report,
        session,
        problems,
    }));
    Ok(())
}

/// The latest state of each item of a run, as the window shows it.
#[derive(Debug, Default)]
pub struct Board {
    pub items: Vec<(ItemId, PathBuf)>,
    pub states: BTreeMap<ItemId, ItemState>,
    pub progress: Option<ProgressBoard>,
    pub notes: Vec<String>,
    pub finished: Option<Finished>,
    pub failure: Option<Failure>,
}

impl Board {
    pub fn take(&mut self, message: Message) {
        match message {
            Message::Started { items, progress } => {
                self.items = items;
                self.progress = Some(progress);
            }
            Message::State { item, state } => {
                self.states.insert(item, state);
            }
            Message::Note(note) => self.notes.push(note),
            Message::Finished(finished) => self.finished = Some(finished),
            Message::Failed(failure) => self.failure = Some(failure),
        }
    }

    pub fn is_over(&self) -> bool {
        self.finished.is_some() || self.failure.is_some()
    }
}
