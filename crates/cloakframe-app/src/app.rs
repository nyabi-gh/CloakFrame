use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::scan::{MediaKind, ScanRequest, ScanResult, scan};
use cloakframe_engine::session::{SessionId, StoredMedia};
use eframe::egui;
use fluent_bundle::FluentValue;

use cloakframe_core::item::{ItemId, ItemState};

use crate::fonts;
use crate::i18n::{Language, Translator};
use crate::models::Models;
use crate::progress::Left;
use crate::run::{self, Board, Failure, Logged, Running};
use crate::sessions::{self, SavedSessions};
use crate::settings::{Method, Notice, Settings, SettingsFile, Theme};
use crate::shell;
use crate::summary;
use crate::thumbnails::Thumbnails;
use crate::tools::Tools;

/// What a menu, a shortcut or a button asks of the app outside the widget it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    AddFiles,
    AddFolder,
    Models,
    About,
    Quit,
}

/// Closing the window while a run works: the run is cancelled first, so the items it holds for
/// review are saved before the app ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quit {
    No,
    Asking,
    /// Cancelled; the window closes once the run has stopped.
    AfterRun,
}

/// A scan of the inputs, on its own thread so the window keeps drawing.
struct Scanning {
    result: mpsc::Receiver<ScanResult>,
    cancel: CancelToken,
}

impl Drop for Scanning {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

pub struct AppState {
    tr: Translator,
    pub settings: Settings,
    file: Option<SettingsFile>,
    notices: Vec<String>,
    pub inputs: Vec<PathBuf>,
    scanning: Option<Scanning>,
    pub found: Option<ScanResult>,
    pub about_open: bool,
    commands: Vec<Command>,
    /// The fonts were made for this language; a change of language makes them again.
    fonts_for: Option<Language>,
    pub tools: Tools,
    pub folders: run::Folders,
    running: Option<Running>,
    thumbnails: Option<Thumbnails>,
    pub models: Models,
    /// The run in progress or the last one, until the user starts anew.
    pub board: Option<Board>,
    pub sessions: SavedSessions,
    sessions_read: bool,
    /// The saved session the window shows instead of the main screen.
    pub opened: Option<SessionId>,
    /// The saved session the user asked to discard, until they confirm.
    pub discarding: Option<SessionId>,
    pub quit: Quit,
}

impl AppState {
    /// `file` is None where nothing is saved, as in tests.
    pub fn new(settings: Settings, file: Option<SettingsFile>, notices: Vec<Notice>) -> Self {
        let language = settings.language.unwrap_or_else(Language::of_system);
        let tr = Translator::new(language);
        let notices = notices.iter().map(|n| describe(&tr, n)).collect();
        Self {
            tr,
            settings,
            file,
            notices,
            inputs: Vec::new(),
            scanning: None,
            found: None,
            about_open: false,
            commands: Vec::new(),
            fonts_for: None,
            tools: Tools::default(),
            folders: run::Folders::default(),
            running: None,
            thumbnails: None,
            models: Models::new(None, false),
            board: None,
            sessions: SavedSessions::new(run::Folders::default()),
            sessions_read: false,
            opened: None,
            discarding: None,
            quit: Quit::No,
        }
    }

    pub fn with_tools(mut self, tools: Tools, folders: run::Folders) -> Self {
        self.tools = tools;
        self.set_folders(folders);
        self
    }

    pub fn set_folders(&mut self, folders: run::Folders) {
        self.models = Models::new(folders.models.clone(), folders.app_models);
        self.sessions = SavedSessions::new(folders.clone());
        self.sessions_read = false;
        self.folders = folders;
    }

    pub fn tr(&self) -> &Translator {
        &self.tr
    }

    pub fn command(&mut self, command: Command) {
        self.commands.push(command);
    }

    pub fn add_inputs(&mut self, paths: impl IntoIterator<Item = PathBuf>, ctx: &egui::Context) {
        let before = self.inputs.len();
        for path in paths {
            if !self.inputs.contains(&path) {
                self.inputs.push(path);
            }
        }
        if self.inputs.len() != before {
            self.rescan(ctx);
        }
    }

    pub fn clear_inputs(&mut self) {
        self.inputs.clear();
        self.scanning = None;
        self.found = None;
        self.thumbnails = None;
    }

    fn rescan(&mut self, ctx: &egui::Context) {
        let request = ScanRequest {
            inputs: self.inputs.clone(),
            recursive: true,
            include_videos: self.tools.videos(),
            output: self.settings.output.clone(),
        };
        let cancel = CancelToken::new();
        let (send, result) = mpsc::channel();
        let (token, repaint) = (cancel.clone(), ctx.clone());
        let spawned = std::thread::Builder::new()
            .name("cloakframe-scan".into())
            .spawn(move || {
                if let Ok(found) = scan(&request, &token) {
                    let _ = send.send(found);
                    repaint.request_repaint();
                }
            });
        match spawned {
            Ok(_) => self.scanning = Some(Scanning { result, cancel }),
            Err(e) => self.notices.push(e.to_string()),
        }
    }

    /// Waits for the scan in progress, for tests.
    pub fn wait_for_scan(&mut self) {
        if let Some(scanning) = self.scanning.take()
            && let Ok(found) = scanning.result.recv()
        {
            self.found = Some(found);
        }
    }

    pub fn save_settings(&mut self) {
        if let Some(file) = &mut self.file
            && let Err(e) = file.save(&self.settings)
        {
            self.notices.push(format!("{}: {e}", file.path().display()));
        }
    }

    fn set_language(&mut self, choice: Option<Language>) {
        self.settings.language = choice;
        self.tr = Translator::new(choice.unwrap_or_else(Language::of_system));
        self.save_settings();
    }

    /// Fonts for the current language. New fonts take effect from the next frame, so the window
    /// installs them before its first.
    pub fn install_fonts(&mut self, ctx: &egui::Context) {
        ctx.set_fonts(fonts::definitions(self.tr.language()));
        self.fonts_for = Some(self.tr.language());
    }

    /// The whole window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.fonts_for != Some(self.tr.language()) {
            self.install_fonts(&ctx);
        }
        ctx.set_theme(match self.settings.theme {
            Theme::System => egui::ThemePreference::System,
            Theme::Light => egui::ThemePreference::Light,
            Theme::Dark => egui::ThemePreference::Dark,
        });
        if let Some(scanning) = &self.scanning
            && let Ok(found) = scanning.result.try_recv()
        {
            self.found = Some(found);
            self.scanning = None;
        }
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.add_inputs(dropped, &ctx);
        }
        for command in std::mem::take(&mut self.commands) {
            self.run(command, &ctx);
        }
        self.take_run_messages();
        self.models.take(&ctx);
        if !self.sessions_read {
            self.sessions_read = true;
            self.sessions.reload(&ctx);
        }
        self.sessions.take();
        self.watch_close(&ctx);

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        egui::Panel::top("menu").show(ui, |ui| crate::menu::bar(ui, self));
        egui::Panel::bottom("start").show(ui, |ui| self.start_row(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            for notice in &self.notices {
                ui.colored_label(ui.visuals().warn_fg_color, notice);
            }
            if self.board.is_some() {
                self.run_view(ui);
            } else if self.opened.is_some() {
                self.session_view(ui);
            } else {
                self.saved_sessions(ui);
                self.drop_area(ui, &ctx);
                ui.separator();
                self.what_and_how(ui);
            }
        });
        crate::about::window(&ctx, self);
        self.models.window(&ctx, &self.tr);
        self.discard_dialog(&ctx);
        self.quit_dialog(&ctx);
    }

    /// Keeps the window open while a run works, until the user agrees to cancel it.
    fn watch_close(&mut self, ctx: &egui::Context) {
        let working = self.running.is_some();
        if working && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.quit == Quit::No {
                self.quit = Quit::Asking;
            }
        }
        if !working && self.quit != Quit::No {
            if self.quit == Quit::AfterRun {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            self.quit = Quit::No;
        }
    }

    fn quit_dialog(&mut self, ctx: &egui::Context) {
        if self.quit != Quit::Asking {
            return;
        }
        let tr = &self.tr;
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::unique("quit")).show(ctx, |ui| {
            ui.heading(tr.get("quit-title"));
            ui.label(tr.get("quit-body"));
            ui.horizontal(|ui| {
                if ui.button(tr.get("quit-confirm")).clicked() {
                    choice = Some(true);
                }
                if ui.button(tr.get("quit-keep")).clicked() {
                    choice = Some(false);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(false);
        }
        match choice {
            Some(true) => {
                if let Some(running) = &self.running {
                    running.cancel();
                }
                self.quit = Quit::AfterRun;
            }
            Some(false) => self.quit = Quit::No,
            None => {}
        }
    }

    fn run(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            Command::AddFiles => {
                if let Some(files) = rfd::FileDialog::new().pick_files() {
                    self.add_inputs(files, ctx);
                }
            }
            Command::AddFolder => {
                if let Some(folders) = rfd::FileDialog::new().pick_folders() {
                    self.add_inputs(folders, ctx);
                }
            }
            Command::Models => self.models.show(ctx),
            Command::About => self.about_open = true,
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn drop_area(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let hovering = ctx.input(|i| i.raw.hovered_files.len());
        let hint = if hovering > 0 {
            self.tr
                .with("drop-release", &[("count", FluentValue::from(hovering))])
        } else {
            self.tr.get("drop-hint")
        };
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_height(120.0);
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.heading(hint);
                ui.horizontal(|ui| {
                    if ui.button(self.tr.get("add-files")).clicked() {
                        self.command(Command::AddFiles);
                    }
                    if ui.button(self.tr.get("add-folder")).clicked() {
                        self.command(Command::AddFolder);
                    }
                    if !self.inputs.is_empty() && ui.button(self.tr.get("clear-list")).clicked() {
                        self.clear_inputs();
                    }
                });
            });
        });
        if self.scanning.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(self.tr.get("scanning"));
            });
        }
        if !self.tools.videos() {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                self.tr.get("videos-need-ffmpeg"),
            );
        }
        let Some(found) = &self.found else {
            return;
        };
        let count = |kind| found.files.iter().filter(|f| f.kind == kind).count();
        ui.label(self.tr.with(
            "found-summary",
            &[
                ("photos", FluentValue::from(count(MediaKind::Image))),
                ("videos", FluentValue::from(count(MediaKind::Video))),
            ],
        ));
        for (extension, count) in &found.skipped {
            let count = FluentValue::from(*count);
            let text = if extension.is_empty() {
                self.tr.with("skipped-no-extension", &[("count", count)])
            } else {
                self.tr.with(
                    "skipped-type",
                    &[
                        ("count", count),
                        ("extension", FluentValue::from(extension.as_str())),
                    ],
                )
            };
            ui.colored_label(ui.visuals().warn_fg_color, text);
        }
        for issue in &found.issues {
            let path = issue.path.display().to_string();
            ui.colored_label(
                ui.visuals().warn_fg_color,
                self.tr
                    .with("unreadable", &[("path", FluentValue::from(path))]),
            );
        }
        if self.thumbnails.is_none() {
            let video = self.tools.ffmpeg.clone().zip(self.tools.ffprobe.clone());
            match Thumbnails::new(ctx, video) {
                Ok(thumbnails) => self.thumbnails = Some(thumbnails),
                Err(e) => self.notices.push(e.to_string()),
            }
        }
        if let Some(thumbnails) = &mut self.thumbnails {
            thumbnails.take(ctx);
        }
        let thumbnails = &mut self.thumbnails;
        egui::ScrollArea::vertical()
            .max_height(280.0)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for file in &found.files {
                        let (cell, _) =
                            ui.allocate_exact_size(egui::vec2(128.0, 148.0), egui::Sense::hover());
                        if !ui.is_rect_visible(cell) {
                            continue;
                        }
                        let picture = egui::Rect::from_min_size(cell.min, egui::vec2(128.0, 128.0));
                        let shown = thumbnails
                            .as_mut()
                            .and_then(|t| t.get(&file.path, file.kind).flatten());
                        match shown {
                            Some(texture) => {
                                let size = texture.size_vec2();
                                let fit = size * (128.0 / size.max_elem());
                                let at = egui::Rect::from_center_size(picture.center(), fit);
                                egui::Image::new(texture).paint_at(ui, at);
                            }
                            None => {
                                let mark = match file.kind {
                                    MediaKind::Image => "▣",
                                    MediaKind::Video => "▶",
                                };
                                ui.painter()
                                    .rect_filled(picture, 4.0, ui.visuals().faint_bg_color);
                                ui.painter().text(
                                    picture.center(),
                                    egui::Align2::CENTER_CENTER,
                                    mark,
                                    egui::FontId::proportional(28.0),
                                    ui.visuals().weak_text_color(),
                                );
                            }
                        }
                        let label = egui::Rect::from_min_max(
                            egui::pos2(cell.min.x, picture.max.y + 2.0),
                            cell.max,
                        );
                        let name = file.relative.display().to_string();
                        ui.put(
                            label,
                            egui::Label::new(egui::RichText::new(name).small()).truncate(),
                        );
                    }
                });
            });
    }

    fn what_and_how(&mut self, ui: &mut egui::Ui) {
        let before = self.settings.clone();
        ui.horizontal_wrapped(|ui| {
            ui.label(self.tr.get("what"));
            ui.checkbox(&mut self.settings.faces, self.tr.get("faces"));
            ui.checkbox(&mut self.settings.plates, self.tr.get("plates"));
            ui.separator();
            ui.label(self.tr.get("how"));
            for (method, id) in [
                (Method::Fill, "method-fill"),
                (Method::Mosaic, "method-mosaic"),
                (Method::Blur, "method-blur"),
            ] {
                ui.radio_value(&mut self.settings.method, method, self.tr.get(id));
            }
            if self.settings.method == Method::Mosaic {
                use cloakframe_core::mask::MosaicStrength;
                ui.add(
                    egui::Slider::new(
                        &mut self.settings.mosaic_strength,
                        MosaicStrength::MIN..=MosaicStrength::MAX,
                    )
                    .text(self.tr.get("strength")),
                );
            }
        });
        ui.checkbox(&mut self.settings.review, self.tr.get("review-first"));
        if !self.settings.faces && !self.settings.plates {
            ui.colored_label(ui.visuals().warn_fg_color, self.tr.get("models-need-one"));
        }
        if self.settings != before {
            self.save_settings();
        }
    }

    fn start_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (view, theme, language) = (
                self.tr.get("menu-view"),
                self.tr.get("theme"),
                self.tr.get("language"),
            );
            ui.menu_button(view, |ui| {
                ui.label(theme);
                self.theme_choice(ui);
                ui.separator();
                ui.label(language);
                self.language_choice(ui);
            });
            ui.separator();
            ui.label(self.tr.get("output-folder"));
            let shown = match &self.settings.output {
                Some(folder) => folder.display().to_string(),
                None => self.tr.get("no-output-folder"),
            };
            ui.label(shown);
            if ui.button(self.tr.get("models-open")).clicked() {
                self.command(Command::Models);
            }
            if ui.button(self.tr.get("choose-folder")).clicked()
                && let Some(folder) = rfd::FileDialog::new().pick_folder()
            {
                self.settings.output = Some(folder);
                self.save_settings();
                if !self.inputs.is_empty() {
                    let ctx = ui.ctx().clone();
                    self.rescan(&ctx);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let blocker = self.start_blocker();
                let start =
                    ui.add_enabled(blocker.is_none(), egui::Button::new(self.tr.get("start")));
                if start.clicked() {
                    self.start_run(ui.ctx());
                }
                if let Some(why) = blocker {
                    start.on_disabled_hover_text(self.tr.get(why));
                }
            });
        });
    }

    /// The message saying why the run cannot start yet.
    pub fn start_blocker(&self) -> Option<&'static str> {
        if self.board.is_some() {
            Some("running")
        } else if !self.settings.faces && !self.settings.plates {
            Some("models-need-one")
        } else if self.found.as_ref().is_none_or(|f| f.files.is_empty()) {
            Some("start-needs-files")
        } else if self.settings.output.is_none() {
            Some("start-needs-output")
        } else if self.tools.ort_library.is_none() {
            Some("start-needs-runtime")
        } else {
            None
        }
    }

    fn start_run(&mut self, ctx: &egui::Context) {
        let (Some(found), Some(output)) = (&self.found, &self.settings.output) else {
            return;
        };
        let request = run::Request {
            files: found.files.clone(),
            output: output.clone(),
            settings: self.settings.clone(),
            tools: self.tools.clone(),
            folders: self.folders.clone(),
        };
        match run::start(request, ctx) {
            Ok(running) => {
                self.running = Some(running);
                self.board = Some(Board::new(Instant::now()));
                self.opened = None;
            }
            Err(e) => self.notices.push(e.to_string()),
        }
    }

    fn take_run_messages(&mut self) {
        let (Some(running), Some(board)) = (&self.running, &mut self.board) else {
            return;
        };
        for message in running.messages() {
            board.take(message);
        }
        if board.is_over() {
            self.running = None;
            self.sessions_read = false;
        }
    }

    /// Waits for the run in progress to end, for tests.
    pub fn wait_for_run(&mut self) {
        let (Some(running), Some(board)) = (&self.running, &mut self.board) else {
            return;
        };
        while !board.is_over() {
            match running.wait() {
                Some(message) => board.take(message),
                None => break,
            }
        }
        self.running = None;
        self.sessions_read = false;
    }

    /// Waits for the saved sessions being read, for tests.
    pub fn wait_for_sessions(&mut self) {
        self.sessions.wait();
    }

    fn show(&mut self, path: &Path, open: bool) {
        let result = if open {
            shell::open(path)
        } else {
            shell::reveal(path)
        };
        if let Err(e) = result {
            let path = path.display().to_string();
            self.notices.push(self.tr.with(
                "shell-failed",
                &[
                    ("path", FluentValue::from(path)),
                    ("error", FluentValue::from(e.to_string())),
                ],
            ));
        }
    }

    fn run_view(&mut self, ui: &mut egui::Ui) {
        let Some(board) = &mut self.board else {
            return;
        };
        let (fraction, left) = board.measure(Instant::now());
        let board = &*board;
        let tr = &self.tr;
        let mut act = Act::None;
        ui.horizontal(|ui| {
            if let Some(finished) = &board.finished {
                let summary =
                    summary::summarize(finished.report.items.iter().map(|o| summary::Item {
                        state: o.state,
                        evidence: &o.analysis.evidence,
                        video: o.analysis.video.as_ref().map(|v| (v.frames, v.seconds)),
                    }));
                ui.heading(summary::sentence(&summary, tr));
            } else if board.failure.is_none() {
                ui.spinner();
                ui.heading(tr.get("running"));
            }
            if board.is_over() {
                if ui.button(tr.get("run-new")).clicked() {
                    act = Act::NewRun;
                }
            } else if self.quit == Quit::AfterRun {
                ui.label(tr.get("quit-waiting"));
            } else if ui.button(tr.get("cancel")).clicked() {
                act = Act::CancelRun;
            }
        });
        if !board.is_over() && board.progress.is_some() {
            let percent = FluentValue::from((fraction * 100.0).floor() as u64);
            let text = format!(
                "{} · {}",
                tr.with("progress-percent", &[("percent", percent)]),
                time_left(tr, left)
            );
            ui.add(egui::ProgressBar::new(fraction).text(text));
        }
        if let Some(failure) = &board.failure {
            let text = match failure {
                Failure::NoRuntime => tr.get("start-needs-runtime"),
                Failure::MissingModel(name) => {
                    tr.with("model-missing", &[("name", FluentValue::from(*name))])
                }
                Failure::Other(e) => e.clone(),
            };
            ui.colored_label(ui.visuals().error_fg_color, text);
            if matches!(failure, Failure::MissingModel(_))
                && ui.button(tr.get("models-open")).clicked()
            {
                act = Act::Models;
            }
        }
        if board.finished.as_ref().is_some_and(|f| f.session.is_some()) {
            ui.label(tr.get("run-session-kept"));
        }
        for note in board
            .notes
            .iter()
            .chain(board.finished.iter().flat_map(|f| f.problems.iter()))
        {
            ui.colored_label(ui.visuals().warn_fg_color, note);
        }
        let progress = board
            .progress
            .as_ref()
            .map(|p| p.snapshot())
            .unwrap_or_default();
        let outcomes = board.finished.as_ref().map(|f| &f.report.items);
        egui::ScrollArea::vertical()
            .max_height((ui.available_height() - 40.0).max(120.0))
            .show(ui, |ui| {
                for (item, input) in &board.items {
                    let state = board.states.get(item).copied().unwrap_or(ItemState::Queued);
                    let outcome = outcomes.and_then(|o| o.iter().find(|o| o.item == *item));
                    let cancelling = board.cancelling.contains(item);
                    ui.horizontal(|ui| {
                        let shown = if cancelling {
                            "state-cancelling"
                        } else {
                            state_id(state)
                        };
                        ui.label(tr.get(shown));
                        ui.label(board.name(*item));
                        let working = !state.is_terminal() && state != ItemState::NeedsReview;
                        if working && let Some(p) = progress.get(item).filter(|p| p.total > 0) {
                            ui.add(
                                egui::ProgressBar::new(p.done as f32 / p.total as f32)
                                    .desired_width(160.0),
                            );
                        }
                        if working
                            && !cancelling
                            && !board.is_over()
                            && ui.button(tr.get("cancel")).clicked()
                        {
                            act = Act::CancelItem(*item);
                        }
                        if let Some(o) = outcome {
                            if matches!(o.state, ItemState::Published(_)) {
                                if ui.button(tr.get("item-reveal")).clicked() {
                                    act = Act::Show(o.destination.clone(), false);
                                }
                                if ui.button(tr.get("item-open")).clicked() {
                                    act = Act::Show(o.destination.clone(), true);
                                }
                            } else if o.state == ItemState::NeedsReview
                                && ui.button(tr.get("item-reveal-source")).clicked()
                            {
                                act = Act::Show(input.clone(), false);
                            }
                        }
                    });
                    if let Some(o) = outcome {
                        ui.indent(("why", item.0), |ui| {
                            if let Some(failure) = &o.failure {
                                ui.colored_label(ui.visuals().error_fg_color, failure);
                            }
                            if o.state == ItemState::NeedsReview {
                                let video =
                                    o.analysis.video.as_ref().map(|v| (v.frames, v.seconds));
                                for e in o.analysis.evidence.iter().filter(|e| !e.is_resolved()) {
                                    ui.weak(summary::problem(e, video, tr));
                                }
                            }
                        });
                    }
                }
            });
        log_panel(ui, board, tr);
        match act {
            Act::None => {}
            Act::CancelRun => {
                if let Some(running) = &self.running {
                    running.cancel();
                }
            }
            Act::CancelItem(item) => {
                if let Some(board) = &mut self.board {
                    board.cancel_item(item);
                }
            }
            Act::NewRun => self.board = None,
            Act::Models => self.command(Command::Models),
            Act::Show(path, open) => self.show(&path, open),
        }
    }

    /// The sessions earlier runs left for review, above the drop area.
    fn saved_sessions(&mut self, ui: &mut egui::Ui) {
        if let Some(e) = &self.sessions.error {
            ui.colored_label(ui.visuals().warn_fg_color, e);
        }
        if self.sessions.list.is_empty() {
            return;
        }
        let tr = &self.tr;
        let now = SystemTime::now();
        let mut open = None;
        let mut discard = None;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.heading(tr.get("sessions-title"));
            for saved in &self.sessions.list {
                ui.horizontal(|ui| {
                    if let Some(created) = saved.created {
                        let days = sessions::age_days(created, now);
                        ui.label(tr.with("session-age", &[("days", FluentValue::from(days))]));
                    }
                    match &saved.content {
                        Ok(loaded) => {
                            let output = loaded.session.output.display().to_string();
                            ui.label(tr.with(
                                "session-row",
                                &[
                                    ("items", FluentValue::from(loaded.session.items.len())),
                                    ("output", FluentValue::from(output)),
                                ],
                            ));
                            let changed = loaded.changed_count();
                            if changed > 0 {
                                ui.colored_label(
                                    ui.visuals().warn_fg_color,
                                    tr.with(
                                        "session-changed",
                                        &[("count", FluentValue::from(changed))],
                                    ),
                                );
                            }
                            if ui.button(tr.get("session-open")).clicked() {
                                open = Some(saved.id.clone());
                            }
                        }
                        Err(e) => {
                            ui.colored_label(
                                ui.visuals().warn_fg_color,
                                tr.with(
                                    "session-unreadable",
                                    &[("error", FluentValue::from(e.as_str()))],
                                ),
                            );
                        }
                    }
                    if ui.button(tr.get("session-discard")).clicked() {
                        discard = Some(saved.id.clone());
                    }
                });
            }
        });
        if open.is_some() {
            self.opened = open;
        }
        if discard.is_some() {
            self.discarding = discard;
        }
    }

    /// A saved session's items and what each waits for, in place of the main screen.
    fn session_view(&mut self, ui: &mut egui::Ui) {
        let tr = &self.tr;
        let Some(saved) = self
            .opened
            .as_ref()
            .and_then(|id| self.sessions.list.iter().find(|s| &s.id == id))
        else {
            self.opened = None;
            return;
        };
        let Ok(loaded) = &saved.content else {
            self.opened = None;
            return;
        };
        let session = &loaded.session;
        let video = |media: &StoredMedia| match media {
            StoredMedia::Video {
                frames, seconds, ..
            } => Some((*frames, *seconds)),
            StoredMedia::Photo { .. } => None,
        };
        let mut act = Act::None;
        ui.horizontal(|ui| {
            if ui.button(tr.get("session-back")).clicked() {
                act = Act::NewRun;
            }
            let summary = summary::summarize(session.items.iter().map(|i| summary::Item {
                state: ItemState::NeedsReview,
                evidence: &i.evidence,
                video: video(&i.media),
            }));
            ui.heading(summary::sentence(&summary, tr));
        });
        ui.horizontal(|ui| {
            ui.label(tr.get("output-folder"));
            ui.label(session.output.display().to_string());
        });
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (item, changed) in session.items.iter().zip(&loaded.changed) {
                ui.horizontal(|ui| {
                    if *changed {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            tr.get("session-item-changed"),
                        );
                    } else {
                        ui.label(tr.get("state-needs-review"));
                    }
                    let name = item
                        .input
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned());
                    ui.label(name.unwrap_or_default());
                    if !*changed && ui.button(tr.get("item-reveal-source")).clicked() {
                        act = Act::Show(item.input.clone(), false);
                    }
                });
                ui.indent(("why", item.item.0), |ui| {
                    for e in item.evidence.iter().filter(|e| !e.is_resolved()) {
                        ui.weak(summary::problem(e, video(&item.media), tr));
                    }
                });
            }
        });
        let discard = ui.button(tr.get("session-discard")).clicked();
        if discard {
            self.discarding = Some(saved.id.clone());
        }
        match act {
            Act::NewRun => self.opened = None,
            Act::Show(path, open) => self.show(&path, open),
            _ => {}
        }
    }

    fn discard_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.discarding.clone() else {
            return;
        };
        let tr = &self.tr;
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::unique("discard")).show(ctx, |ui| {
            ui.heading(tr.get("discard-title"));
            ui.label(tr.get("discard-body"));
            ui.horizontal(|ui| {
                if ui.button(tr.get("discard-confirm")).clicked() {
                    choice = Some(true);
                }
                if ui.button(tr.get("discard-keep")).clicked() {
                    choice = Some(false);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(false);
        }
        match choice {
            Some(true) => {
                if let Err(e) = self.sessions.discard(&id) {
                    self.notices.push(e.to_string());
                }
                if self.opened.as_ref() == Some(&id) {
                    self.opened = None;
                }
                self.discarding = None;
            }
            Some(false) => self.discarding = None,
            None => {}
        }
    }

    pub fn theme_choice(&mut self, ui: &mut egui::Ui) {
        let before = self.settings.theme;
        for (theme, id) in [
            (Theme::System, "theme-system"),
            (Theme::Light, "theme-light"),
            (Theme::Dark, "theme-dark"),
        ] {
            ui.radio_value(&mut self.settings.theme, theme, self.tr.get(id));
        }
        if self.settings.theme != before {
            self.save_settings();
        }
    }

    pub fn language_choice(&mut self, ui: &mut egui::Ui) {
        let current = self.settings.language;
        let mut choice = current;
        ui.radio_value(&mut choice, None, self.tr.get("language-system"));
        for language in Language::ALL {
            ui.radio_value(&mut choice, Some(language), language.name());
        }
        if choice != current {
            self.set_language(choice);
        }
    }
}

fn describe(tr: &Translator, notice: &Notice) -> String {
    match notice {
        Notice::UnknownKeys(keys) => tr.with(
            "settings-unknown-keys",
            &[("keys", FluentValue::from(keys.join(", ")))],
        ),
        Notice::Unreadable(error) => tr.with(
            "settings-unreadable",
            &[("error", FluentValue::from(error.as_str()))],
        ),
        Notice::Newer(version) => tr.with(
            "settings-newer",
            &[("version", FluentValue::from(*version))],
        ),
    }
}

/// What a click asked for, done once drawing no longer borrows the state.
enum Act {
    None,
    CancelRun,
    CancelItem(ItemId),
    NewRun,
    Models,
    /// Show in the file manager, or open with `true`.
    Show(PathBuf, bool),
}

fn time_left(tr: &Translator, left: Option<Duration>) -> String {
    match left.map(Left::of) {
        None => tr.get("progress-left-estimating"),
        Some(Left::UnderAMinute) => tr.get("progress-left-under-minute"),
        Some(Left::Minutes(minutes)) => tr.with(
            "progress-left-minutes",
            &[("minutes", FluentValue::from(minutes))],
        ),
        Some(Left::Hours { hours, minutes }) => tr.with(
            "progress-left-hours",
            &[
                ("hours", FluentValue::from(hours)),
                ("minutes", FluentValue::from(minutes)),
            ],
        ),
    }
}

/// One log line: time since the start, the item's name, and what happened.
pub fn log_line(board: &Board, at: Duration, logged: &Logged, tr: &Translator) -> String {
    let secs = at.as_secs();
    let time = format!("{:02}:{:02}", secs / 60, secs % 60);
    let (item, text) = match logged {
        Logged::State(item, state) => (Some(*item), tr.get(state_id(*state))),
        Logged::Note(item, note) => (*item, note.clone()),
        Logged::Failed(item, failure) => (Some(*item), failure.clone()),
    };
    match item {
        Some(item) => format!("{time}  {}  {text}", board.name(item)),
        None => format!("{time}  {text}"),
    }
}

/// The run's log, folded away under the list (plan 6).
fn log_panel(ui: &mut egui::Ui, board: &Board, tr: &Translator) {
    egui::CollapsingHeader::new(tr.get("log-title"))
        .id_salt("log")
        .show(ui, |ui| {
            let lines: Vec<String> = board
                .log
                .iter()
                .map(|(at, logged)| log_line(board, *at, logged, tr))
                .collect();
            if ui.button(tr.get("log-copy")).clicked() {
                ui.ctx().copy_text(lines.join("\n"));
            }
            egui::ScrollArea::vertical()
                .id_salt("log-lines")
                .max_height(200.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in &lines {
                        ui.monospace(line);
                    }
                });
        });
}

fn state_id(state: ItemState) -> &'static str {
    match state {
        ItemState::Queued => "state-queued",
        ItemState::Analyzed | ItemState::ReadyForRender(_) => "state-analyzed",
        ItemState::NeedsReview => "state-needs-review",
        ItemState::Rendering(_) => "state-rendering",
        ItemState::Verified(_) => "state-verified",
        ItemState::Published(_) => "state-published",
        ItemState::Failed => "state-failed",
        ItemState::Cancelled => "state-cancelled",
    }
}
