//! The NVIDIA GPU pack in the advanced options sheet on Linux (docs/M7_ACCELERATORS.md): what it
//! is, NVIDIA's terms, which the user accepts before the download, the download on its own
//! thread, and removing the pack.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};

use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::gpu_pack::{self, GpuPack, Status};
use cloakframe_engine::model_files::download::Https;
use eframe::egui;
use fluent_bundle::FluentValue;

use crate::i18n::Translator;
use crate::models::SharedFetch;
use crate::style::{self, Palette};

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

pub struct GpuPackRow {
    pack: Option<GpuPack>,
    fetch: SharedFetch,
    pub status: Option<Status>,
    pub accepted: bool,
    download: Option<Download>,
    pub error: Option<String>,
}

impl GpuPackRow {
    pub fn new(pack: Option<GpuPack>) -> Self {
        Self::with_fetch(pack, Arc::new(Https::gpu_pack()))
    }

    pub fn with_fetch(pack: Option<GpuPack>, fetch: SharedFetch) -> Self {
        Self {
            pack,
            fetch,
            status: None,
            accepted: false,
            download: None,
            error: None,
        }
    }

    /// Looks again at the pack's folder; a few file sizes, so on the window's thread.
    pub fn check(&mut self) {
        self.status = self.pack.as_ref().map(GpuPack::status);
    }

    pub fn is_busy(&self) -> bool {
        self.download.is_some()
    }

    fn start(&mut self, ctx: &egui::Context) {
        let Some(pack) = self.pack.clone() else {
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
            .name("cloakframe-gpu-pack".into())
            .spawn(move || {
                let result = pack
                    .install(fetch.as_ref(), &token, &mut |n| {
                        count.store(n, Ordering::Relaxed);
                        repaint.request_repaint();
                    })
                    .map_err(|e| e.to_string());
                let _ = send.send(result);
                repaint.request_repaint();
            });
        match spawned {
            Ok(_) => {
                self.error = None;
                self.download = Some(Download {
                    received,
                    cancel,
                    done,
                });
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// Takes the download's end, if it came since the last frame.
    pub fn take(&mut self) {
        if let Some(download) = &self.download
            && let Ok(result) = download.done.try_recv()
        {
            self.finish(result);
        }
    }

    /// Waits for the download in progress, for tests.
    pub fn wait(&mut self) {
        if let Some(download) = &self.download
            && let Ok(result) = download.done.recv()
        {
            self.finish(result);
        }
    }

    fn finish(&mut self, result: Result<(), String>) {
        self.download = None;
        if let Err(e) = result {
            self.error = Some(e);
        }
        self.check();
    }

    /// The pack's row: its size, NVIDIA's terms, and what can be done with it now. `restart` says
    /// the pack is installed but this process loaded ONNX Runtime without it.
    pub fn rows(&mut self, ui: &mut egui::Ui, tr: &Translator, restart: bool) {
        let Some(pack) = self.pack.clone() else {
            return;
        };
        let p = Palette::of(ui);
        let small = |text: String| egui::RichText::new(text).size(12.0).color(p.secondary);
        let megabytes =
            |bytes: u64| FluentValue::from(format!("{:.0}", bytes as f64 / 1_048_576.0));
        let (mut start, mut stop, mut remove) = (false, false, false);
        let ready = self.status == Some(Status::Ready);
        ui.horizontal(|ui| {
            ui.label(tr.get("gpu-pack-name"));
            ui.label(small(tr.with(
                "gpu-pack-size",
                &[
                    ("download", megabytes(pack.download_size())),
                    ("installed", megabytes(gpu_pack::installed_size())),
                ],
            )));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(download) = &self.download {
                    if style::plain(ui, tr.get("cancel")).clicked() {
                        stop = true;
                    }
                    let received = download.received.load(Ordering::Relaxed);
                    ui.add(
                        egui::ProgressBar::new(received as f32 / pack.download_size() as f32)
                            .desired_width(120.0),
                    );
                } else if ready {
                    if style::plain(ui, tr.get("gpu-pack-remove")).clicked() {
                        remove = true;
                    }
                    ui.label(small(tr.get("model-present")));
                } else {
                    let button = egui::Button::new(tr.get("model-download"))
                        .min_size(egui::vec2(0.0, style::BUTTON_HEIGHT))
                        .corner_radius(style::BUTTON_RADIUS);
                    if ui.add_enabled(self.accepted, button).clicked() {
                        start = true;
                    }
                }
            });
        });
        ui.label(small(tr.get("gpu-pack-about")));
        if !ready && self.download.is_none() {
            ui.label(small(tr.get("gpu-pack-terms")));
            for (name, url) in gpu_pack::TERMS {
                ui.hyperlink_to(name, url);
            }
            ui.add(style::Check::new(
                &mut self.accepted,
                tr.get("model-accept-terms"),
            ));
        }
        if let Some(Status::Damaged(reason)) = &self.status {
            ui.colored_label(
                p.needs_review,
                tr.with(
                    "gpu-pack-damaged",
                    &[("reason", FluentValue::from(reason.as_str()))],
                ),
            );
        }
        if ready && restart {
            ui.colored_label(p.needs_review, tr.get("gpu-pack-restart"));
        }
        if let Some(error) = &self.error {
            ui.colored_label(
                p.failed,
                tr.with(
                    "model-download-failed",
                    &[("error", FluentValue::from(error.as_str()))],
                ),
            );
        }
        if start {
            self.start(ui.ctx());
        }
        if stop {
            self.download = None;
        }
        if remove {
            if let Err(e) = pack.remove() {
                self.error = Some(e.to_string());
            }
            self.check();
        }
    }
}
