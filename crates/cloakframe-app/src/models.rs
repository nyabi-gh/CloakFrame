//! The detection models: their terms and size, whether the model folder has each, and
//! downloading one when the user asks (plan 4.6). A model whose terms limit its use is downloaded
//! only after the user accepts them. Checking and downloading run on their own threads.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};

use cloakframe_core::detection::ModelId;
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::model_files::download::{Fetch, Https, download};
use cloakframe_engine::model_files::{MODEL_FILES, ModelStore, Status};
use cloakframe_engine::startup;
use eframe::egui;
use fluent_bundle::FluentValue;

use crate::i18n::Translator;

pub type SharedFetch = Arc<dyn Fetch + Send + Sync>;

struct Download {
    received: Arc<AtomicU64>,
    cancel: CancelToken,
    done: mpsc::Receiver<Result<(), String>>,
}

impl Drop for Download {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

pub struct Models {
    pub open: bool,
    folder: Option<PathBuf>,
    /// The folder is the app's own, which first takes the models 1.12.1 downloaded.
    app_folder: bool,
    fetch: SharedFetch,
    pub statuses: HashMap<ModelId, Result<Status, String>>,
    checking: Option<mpsc::Receiver<HashMap<ModelId, Result<Status, String>>>>,
    downloads: HashMap<ModelId, Download>,
    pub accepted: HashSet<ModelId>,
    pub errors: HashMap<ModelId, String>,
}

impl Models {
    pub fn new(folder: Option<PathBuf>, app_folder: bool) -> Self {
        Self::with_fetch(folder, app_folder, Arc::new(Https::default()))
    }

    pub fn with_fetch(folder: Option<PathBuf>, app_folder: bool, fetch: SharedFetch) -> Self {
        Self {
            open: false,
            folder,
            app_folder,
            fetch,
            statuses: HashMap::new(),
            checking: None,
            downloads: HashMap::new(),
            accepted: HashSet::new(),
            errors: HashMap::new(),
        }
    }

    /// Opens the window and looks again at what the folder holds.
    pub fn show(&mut self, ctx: &egui::Context) {
        self.open = true;
        self.check(ctx);
    }

    fn check(&mut self, ctx: &egui::Context) {
        let Some(folder) = self.folder.clone() else {
            return;
        };
        let (send, result) = mpsc::channel();
        let (app_folder, repaint) = (self.app_folder, ctx.clone());
        let spawned = std::thread::Builder::new()
            .name("cloakframe-models".into())
            .spawn(move || {
                let store = ModelStore::new(folder);
                if app_folder {
                    let all = MODEL_FILES.iter().map(|f| f.model);
                    // A model that cannot be copied is simply missing here; the run says why.
                    let _ = startup::import_legacy(&store, all, &mut |_| {});
                }
                let statuses = MODEL_FILES
                    .iter()
                    .map(|f| (f.model, store.status(f.model).map_err(|e| e.to_string())))
                    .collect();
                let _ = send.send(statuses);
                repaint.request_repaint();
            });
        if spawned.is_ok() {
            self.checking = Some(result);
        }
    }

    pub fn is_busy(&self) -> bool {
        self.checking.is_some() || !self.downloads.is_empty()
    }

    fn start_download(&mut self, model: ModelId, ctx: &egui::Context) {
        let Some(folder) = self.folder.clone() else {
            return;
        };
        let received = Arc::new(AtomicU64::new(0));
        let cancel = CancelToken::new();
        let (send, done) = mpsc::channel();
        let (count, token, fetch, repaint) = (
            received.clone(),
            cancel.clone(),
            self.fetch.clone(),
            ctx.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("cloakframe-download".into())
            .spawn(move || {
                let store = ModelStore::new(folder);
                let result = download(&store, model, fetch.as_ref(), &token, &mut |n| {
                    count.store(n, Ordering::Relaxed);
                    repaint.request_repaint();
                })
                .map_err(|e| e.to_string());
                let _ = send.send(result);
                repaint.request_repaint();
            });
        match spawned {
            Ok(_) => {
                self.errors.remove(&model);
                self.downloads.insert(
                    model,
                    Download {
                        received,
                        cancel,
                        done,
                    },
                );
            }
            Err(e) => {
                self.errors.insert(model, e.to_string());
            }
        }
    }

    /// Takes what the threads finished since the last frame.
    pub fn take(&mut self, ctx: &egui::Context) {
        if let Some(checking) = &self.checking
            && let Ok(statuses) = checking.try_recv()
        {
            self.statuses = statuses;
            self.checking = None;
        }
        let finished: Vec<(ModelId, Result<(), String>)> = self
            .downloads
            .iter()
            .filter_map(|(&model, d)| d.done.try_recv().ok().map(|r| (model, r)))
            .collect();
        for (model, result) in finished {
            self.downloads.remove(&model);
            if let Err(e) = result {
                self.errors.insert(model, e);
            }
            self.check(ctx);
        }
    }

    /// Waits for the check and the downloads in progress, for tests.
    pub fn wait(&mut self, ctx: &egui::Context) {
        while self.is_busy() {
            if let Some(checking) = self.checking.take()
                && let Ok(statuses) = checking.recv()
            {
                self.statuses = statuses;
            }
            let models: Vec<ModelId> = self.downloads.keys().copied().collect();
            for model in models {
                if let Some(d) = self.downloads.remove(&model) {
                    if let Ok(Err(e)) = d.done.recv() {
                        self.errors.insert(model, e);
                    }
                    self.check(ctx);
                }
            }
        }
    }

    pub fn window(&mut self, ctx: &egui::Context, tr: &Translator) {
        let mut open = self.open;
        egui::Window::new(tr.get("models-title"))
            .open(&mut open)
            .default_width(560.0)
            .show(ctx, |ui| self.contents(ui, tr));
        self.open = open;
    }

    fn contents(&mut self, ui: &mut egui::Ui, tr: &Translator) {
        if let Some(folder) = &self.folder {
            let path = folder.display().to_string();
            ui.label(tr.with("models-folder", &[("path", FluentValue::from(path))]));
        }
        if self.checking.is_some() {
            ui.spinner();
        }
        let ctx = ui.ctx().clone();
        let mut start = None;
        let mut stop = None;
        for file in &MODEL_FILES {
            let model = file.model;
            ui.separator();
            ui.horizontal(|ui| {
                ui.strong(tr.get(name_id(model)));
                ui.label(tr.with(
                    "model-size",
                    &[(
                        "size",
                        FluentValue::from(format!("{:.1}", file.size as f64 / 1_048_576.0)),
                    )],
                ));
            });
            ui.label(tr.get(terms_id(model)));
            ui.label(tr.with(
                "model-license",
                &[("license", FluentValue::from(file.terms.license))],
            ));
            let status = match self.statuses.get(&model) {
                Some(Ok(Status::Present)) => tr.get("model-present"),
                Some(Ok(Status::Missing)) => tr.get("model-not-downloaded"),
                Some(Ok(Status::Different)) => tr.get("model-different"),
                Some(Err(e)) => e.clone(),
                None => String::new(),
            };
            ui.label(status);
            if let Some(download) = self.downloads.get(&model) {
                ui.horizontal(|ui| {
                    let received = download.received.load(Ordering::Relaxed);
                    ui.add(egui::ProgressBar::new(received as f32 / file.size as f32));
                    if ui.button(tr.get("cancel")).clicked() {
                        stop = Some(model);
                    }
                });
            } else if !matches!(self.statuses.get(&model), Some(Ok(Status::Present))) {
                if file.terms.non_commercial_only {
                    let mut accepted = self.accepted.contains(&model);
                    if ui
                        .checkbox(&mut accepted, tr.get("model-accept-terms"))
                        .changed()
                    {
                        if accepted {
                            self.accepted.insert(model);
                        } else {
                            self.accepted.remove(&model);
                        }
                    }
                }
                let allowed = !file.terms.non_commercial_only || self.accepted.contains(&model);
                let ready = allowed && self.checking.is_none() && self.folder.is_some();
                if ui
                    .add_enabled(ready, egui::Button::new(tr.get("model-download")))
                    .clicked()
                {
                    start = Some(model);
                }
            }
            if let Some(error) = self.errors.get(&model) {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    tr.with(
                        "model-download-failed",
                        &[("error", FluentValue::from(error.as_str()))],
                    ),
                );
            }
        }
        if let Some(model) = start {
            self.start_download(model, &ctx);
        }
        if let Some(model) = stop {
            self.downloads.remove(&model);
        }
    }
}

pub fn name_id(model: ModelId) -> &'static str {
    match model {
        ModelId::Yolo5FaceN => "model-name-yolo5face-n",
        ModelId::YuNet => "model-name-yunet",
        ModelId::PlateYolov9T => "model-name-plates",
    }
}

pub fn terms_id(model: ModelId) -> &'static str {
    match model {
        ModelId::Yolo5FaceN => "model-terms-yolo5face-n",
        ModelId::YuNet => "model-terms-yunet",
        ModelId::PlateYolov9T => "model-terms-plates",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    /// The English terms are the engine's own words, which the CLI prints.
    #[test]
    fn the_english_terms_are_the_engines() {
        let en = Translator::new(Language::En);
        for file in &MODEL_FILES {
            assert_eq!(en.get(terms_id(file.model)), file.terms.summary);
        }
    }
}
