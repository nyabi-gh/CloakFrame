use std::path::PathBuf;
use std::sync::mpsc;

use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::scan::{MediaKind, ScanRequest, ScanResult, scan};
use eframe::egui;
use fluent_bundle::FluentValue;

use cloakframe_core::item::ItemState;

use crate::fonts;
use crate::i18n::{Language, Translator};
use crate::run::{self, Board, Failure, Running};
use crate::settings::{Method, Notice, Settings, SettingsFile, Theme};
use crate::thumbnails::Thumbnails;
use crate::tools::Tools;

/// What a menu, a shortcut or a button asks of the app outside the widget it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    AddFiles,
    AddFolder,
    About,
    Quit,
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
    /// The run in progress or the last one, until the user starts anew.
    pub board: Option<Board>,
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
            board: None,
        }
    }

    pub fn with_tools(mut self, tools: Tools, folders: run::Folders) -> Self {
        self.tools = tools;
        self.folders = folders;
        self
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

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        egui::Panel::top("menu").show(ui, |ui| crate::menu::bar(ui, self));
        egui::Panel::bottom("start").show(ui, |ui| self.start_row(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            for notice in &self.notices {
                ui.colored_label(ui.visuals().warn_fg_color, notice);
            }
            if self.board.is_some() {
                self.run_view(ui);
            } else {
                self.drop_area(ui, &ctx);
                ui.separator();
                self.what_and_how(ui);
            }
        });
        crate::about::window(&ctx, self);
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
            match Thumbnails::new(ctx) {
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
                        let shown = match (file.kind, thumbnails.as_mut()) {
                            (MediaKind::Image, Some(t)) => t.get(&file.path).flatten(),
                            _ => None,
                        };
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
                self.board = Some(Board::default());
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
    }

    fn run_view(&mut self, ui: &mut egui::Ui) {
        let Some(board) = &self.board else {
            return;
        };
        let tr = &self.tr;
        let mut cancel = false;
        let mut again = false;
        ui.horizontal(|ui| {
            if let Some(finished) = &board.finished {
                let count = |f: fn(ItemState) -> bool| {
                    FluentValue::from(finished.report.items.iter().filter(|o| f(o.state)).count())
                };
                ui.heading(tr.with(
                    "run-summary",
                    &[
                        ("saved", count(|s| matches!(s, ItemState::Published(_)))),
                        ("review", count(|s| s == ItemState::NeedsReview)),
                        ("failed", count(|s| s == ItemState::Failed)),
                    ],
                ));
            } else if board.failure.is_none() {
                ui.spinner();
                ui.heading(tr.get("running"));
            }
            if board.is_over() {
                again = ui.button(tr.get("run-new")).clicked();
            } else {
                cancel = ui.button(tr.get("cancel")).clicked();
            }
        });
        if let Some(failure) = &board.failure {
            let text = match failure {
                Failure::NoRuntime => tr.get("start-needs-runtime"),
                Failure::MissingModel(name) => {
                    tr.with("model-missing", &[("name", FluentValue::from(*name))])
                }
                Failure::Other(e) => e.clone(),
            };
            ui.colored_label(ui.visuals().error_fg_color, text);
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
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (item, input) in &board.items {
                let state = board.states.get(item).copied().unwrap_or(ItemState::Queued);
                ui.horizontal(|ui| {
                    ui.label(tr.get(state_id(state)));
                    let name = input.file_name().map(|n| n.to_string_lossy().into_owned());
                    ui.label(name.unwrap_or_default());
                    if !state.is_terminal()
                        && state != ItemState::NeedsReview
                        && let Some(p) = progress.get(item).filter(|p| p.total > 0)
                    {
                        ui.add(egui::ProgressBar::new(p.done as f32 / p.total as f32));
                    }
                });
            }
        });
        if cancel && let Some(running) = &self.running {
            running.cancel();
        }
        if again {
            self.board = None;
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
