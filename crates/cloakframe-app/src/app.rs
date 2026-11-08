use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::events::Progress;
use cloakframe_engine::run::Note;
use cloakframe_engine::scan::{MediaKind, ScanRequest, ScanResult, scan};
use cloakframe_engine::session::{SessionError, SessionId};
use eframe::egui;
use fluent_bundle::FluentValue;

use cloakframe_core::detection::ModelId;
use cloakframe_core::item::{ItemId, ItemState, PublishBasis};

use crate::a11y;
use crate::clock;
use crate::editor::{ReviewScreen, ScreenAct};
use crate::fonts;
use crate::gpu_pack::GpuPackRow;
use crate::i18n::{Language, Translator};
use crate::models::Models;
use crate::progress::Left;
use crate::review::{OpenError, Review};
use crate::run::{self, Board, Failure, Logged, Running};
use crate::sample::Sample;
use crate::sessions::{Saved, SavedSessions};
use crate::settings::{
    Coverage, FaceModel, Method, Notice, Settings, SettingsFile, Shape, Theme, VideoCodec,
    VideoQuality,
};
use crate::shell;
use crate::style::{self, Palette};
use crate::summary;
use crate::thumbnails::Thumbnails;
use crate::tools::Tools;
use crate::updates::Updates;
use crate::wording;

/// The sample photo's height over its width in the settings panel.
const SAMPLE_ASPECT: f32 = 146.0 / 260.0;
/// The sample photo's height below which it stops giving up room and the panel scrolls instead.
const SAMPLE_MIN_HEIGHT: f32 = 64.0;
const APP_ICON: &[u8] = include_bytes!("../../../assets/icons/cloakframe.png");
const DIALOG_WIDTH: f32 = 720.0;
const DIALOG_MAX_HEIGHT: f32 = 620.0;
const DIALOG_TABS_WIDTH: f32 = 164.0;
/// The least room left above and below the settings dialog.
const DIALOG_MARGIN: f32 = 40.0;
/// As tall as the review's footer, whose primary button sits at the same place.
const PANEL_FOOTER_HEIGHT: f32 = 64.0;
const DIALOG_RADIUS: u8 = 10;

/// The words of the sensitivity slider's steps, least sensitive first.
const SENSITIVITY_LEVELS: [&str; 5] = [
    "sensitivity-lowest",
    "sensitivity-low",
    "sensitivity-default",
    "sensitivity-high",
    "sensitivity-highest",
];

/// What a menu, a shortcut or a button asks of the app outside the widget it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    AddFiles,
    AddFolder,
    Settings,
    /// The settings at their models, for what is missing a model.
    Models,
    About,
    CheckUpdates,
    /// Turns the check at startup on or off.
    CheckUpdatesOnStart(bool),
    Quit,
    Theme(Theme),
    /// None follows the system's language.
    Language(Option<Language>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SettingsTab {
    #[default]
    General,
    Detection,
    Output,
    Models,
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
    /// Shared so that drawing can hold the words while it changes the rest of the state.
    tr: Rc<Translator>,
    pub settings: Settings,
    file: Option<SettingsFile>,
    notices: Vec<String>,
    pub inputs: Vec<PathBuf>,
    /// Files the user took out of the list that a folder in `inputs` would bring back.
    removed: BTreeSet<PathBuf>,
    scanning: Option<Scanning>,
    pub found: Option<ScanResult>,
    pub about_open: bool,
    /// The renderer the window runs on, shown in the About window.
    pub graphics: Option<String>,
    commands: Vec<Command>,
    /// The fonts were made for this language; a change of language makes them again.
    fonts_for: Option<Language>,
    pub tools: Tools,
    pub folders: run::Folders,
    /// Where results go while the user has not picked a folder; None in tests.
    pub default_output: Option<PathBuf>,
    running: Option<Running>,
    thumbnails: Option<Thumbnails>,
    pub models: Models,
    models_checked: bool,
    /// The Linux GPU pack, shown on Linux x86_64 only.
    pub gpu_pack: GpuPackRow,
    pub updates: Updates,
    /// The check at startup has been asked for, or was not wanted.
    updates_started: bool,
    /// The run in progress or the last one, until the user starts anew.
    pub board: Option<Board>,
    pub sessions: SavedSessions,
    sessions_read: bool,
    /// The session under review, which the window shows instead of the other screens.
    pub review: Option<ReviewScreen>,
    /// The saved session the user asked to discard, until they confirm.
    pub discarding: Option<SessionId>,
    pub quit: Quit,
    sample: Sample,
    /// The most height the sample photo may take so that the settings panel fits, from the last
    /// frame; None until measured.
    sample_room: Option<f32>,
    /// The settings dialog is open.
    pub dialog_open: bool,
    pub settings_tab: SettingsTab,
    /// How much of the run is done, drawn as a line under the toolbar.
    progress_line: Option<f32>,
    /// The app's icon at the head of the sidebar, made on first use.
    icon: Option<egui::TextureHandle>,
}

impl AppState {
    /// `file` is None where nothing is saved, as in tests.
    pub fn new(settings: Settings, file: Option<SettingsFile>, notices: Vec<Notice>) -> Self {
        let language = settings.language.unwrap_or_else(Language::of_system);
        let tr = Rc::new(Translator::new(language));
        let notices = notices.iter().map(|n| describe(&tr, n)).collect();
        Self {
            tr,
            settings,
            file,
            notices,
            inputs: Vec::new(),
            removed: BTreeSet::new(),
            scanning: None,
            found: None,
            about_open: false,
            graphics: None,
            commands: Vec::new(),
            fonts_for: None,
            tools: Tools::default(),
            folders: run::Folders::default(),
            default_output: None,
            running: None,
            thumbnails: None,
            models: Models::new(None, false),
            models_checked: false,
            gpu_pack: GpuPackRow::new(None),
            updates: Updates::unavailable(String::new()),
            updates_started: false,
            board: None,
            sessions: SavedSessions::new(run::Folders::default()),
            sessions_read: false,
            review: None,
            discarding: None,
            quit: Quit::No,
            sample: Sample::new(),
            sample_room: None,
            dialog_open: false,
            settings_tab: SettingsTab::default(),
            progress_line: None,
            icon: None,
        }
    }

    pub fn with_tools(mut self, tools: Tools, folders: run::Folders) -> Self {
        self.tools = tools;
        self.set_folders(folders);
        self
    }

    /// The folder the user picked, or else the default one.
    pub fn output(&self) -> Option<&PathBuf> {
        self.settings
            .output
            .as_ref()
            .or(self.default_output.as_ref())
    }

    pub fn set_folders(&mut self, folders: run::Folders) {
        self.models = Models::new(folders.models.clone(), folders.app_models);
        self.models_checked = false;
        self.gpu_pack = GpuPackRow::new(folders.gpu_pack());
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
        let mut restored = false;
        for path in paths {
            restored |= self.removed.remove(&path);
            if !self.inputs.contains(&path) {
                self.inputs.push(path);
            }
        }
        if self.inputs.len() != before || restored {
            self.rescan(ctx);
        }
    }

    /// Takes one file out of the list; with the last one the list starts over.
    pub fn remove_input(&mut self, path: &Path) {
        if let Some(found) = &mut self.found {
            found.files.retain(|f| f.path != path);
            if found.files.is_empty() {
                self.clear_inputs();
                return;
            }
        }
        if self.inputs.iter().any(|input| input == path) {
            self.inputs.retain(|input| input != path);
        } else {
            self.removed.insert(path.to_path_buf());
        }
    }

    fn take_found(&mut self, mut found: ScanResult) {
        found.files.retain(|f| !self.removed.contains(&f.path));
        self.found = Some(found);
    }

    pub fn clear_inputs(&mut self) {
        self.inputs.clear();
        self.removed.clear();
        self.scanning = None;
        self.found = None;
        self.thumbnails = None;
    }

    fn rescan(&mut self, ctx: &egui::Context) {
        let request = ScanRequest {
            inputs: self.inputs.clone(),
            recursive: true,
            include_videos: self.tools.videos(),
            output: self.output().cloned(),
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
            self.take_found(found);
        }
    }

    /// Waits for the thumbnails the file grid asked for, for tests.
    pub fn wait_for_thumbnails(&mut self, ctx: &egui::Context) {
        if let Some(thumbnails) = &mut self.thumbnails {
            thumbnails.wait(ctx);
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
        self.tr = Rc::new(Translator::new(choice.unwrap_or_else(Language::of_system)));
    }

    /// Fonts for the current language. New fonts take effect from the next frame, so the window
    /// installs them before its first.
    pub fn install_fonts(&mut self, ctx: &egui::Context) {
        ctx.set_fonts(fonts::definitions(self.tr.language()));
        style::install(ctx);
        self.fonts_for = Some(self.tr.language());
    }

    /// The whole window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.fonts_for != Some(self.tr.language()) {
            self.install_fonts(&ctx);
        }
        ctx.set_theme(theme_preference(self.settings.theme));
        // What the threads finished comes in before anything below starts a thread, so a result is
        // never taken in the pass that asked for it. Tests wait for a thread between frames.
        if let Some(scanning) = &self.scanning
            && let Ok(found) = scanning.result.try_recv()
        {
            self.take_found(found);
            self.scanning = None;
        }
        self.take_run_messages();
        self.models.take(&ctx);
        self.gpu_pack.take();
        if self.updates.take() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.sessions.take();
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
        if !self.sessions_read {
            self.sessions_read = true;
            self.sessions.reload(&ctx);
        }
        if !self.models_checked {
            self.models_checked = true;
            self.models.check(&ctx);
        }
        if !self.updates_started {
            self.updates_started = true;
            if self.settings.check_updates {
                self.updates.check(false, &ctx);
            }
        }
        self.watch_close(&ctx);

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        egui::Panel::top("menu").show(ui, |ui| crate::menu::bar(ui, self));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.window(ui));
        crate::about::window(&ctx, self);
        self.settings_dialog(&ctx);
        self.discard_dialog(&ctx);
        self.quit_dialog(&ctx);
    }

    fn screen(&self) -> Screen {
        if self.review.is_some() {
            Screen::Review
        } else if self.board.is_some() {
            Screen::Run
        } else {
            Screen::Main
        }
    }

    /// The toolbar, the sidebar, the middle and the settings panel. They are laid out by hand
    /// rather than as egui panels so that Tab goes through them as they read: egui moves focus in
    /// the order widgets are made, and side panels have to be made before the middle.
    fn window(&mut self, ui: &mut egui::Ui) {
        let p = *Palette::of(ui);
        let whole = ui.max_rect();
        let screen = self.screen();
        let sidebar = self.sidebar_shown();
        let settings = screen == Screen::Main;
        let below_bar = whole.top() + style::TOOLBAR_HEIGHT;
        let sidebar_right = whole.left() + if sidebar { style::SIDEBAR_WIDTH } else { 0.0 };
        let settings_left = whole.right()
            - if settings {
                style::INSPECTOR_WIDTH
            } else {
                0.0
            };
        let bar = egui::Rect::from_x_y_ranges(whole.x_range(), whole.top()..=below_bar);
        let side =
            egui::Rect::from_x_y_ranges(whole.left()..=sidebar_right, below_bar..=whole.bottom());
        let middle =
            egui::Rect::from_x_y_ranges(sidebar_right..=settings_left, below_bar..=whole.bottom());
        let panel =
            egui::Rect::from_x_y_ranges(settings_left..=whole.right(), below_bar..=whole.bottom());

        let painter = ui.painter();
        painter.rect_filled(whole, 0.0, p.window);
        if sidebar {
            let column = egui::Rect::from_x_y_ranges(whole.left()..=sidebar_right, whole.y_range());
            painter.rect_filled(column, 0.0, p.sidebar);
        }
        let empty =
            screen == Screen::Main && self.found.as_ref().is_none_or(|f| f.files.is_empty());
        if empty {
            painter.rect_filled(middle, 0.0, p.empty);
        }

        if sidebar {
            let head = egui::Rect::from_x_y_ranges(whole.left()..=sidebar_right, bar.y_range());
            self.sidebar_head(ui, head);
        }
        region(ui, bar, |ui| {
            self.toolbar(ui, if sidebar { sidebar_right } else { whole.left() })
        });
        if sidebar {
            region(ui, side, |ui| self.sidebar(ui));
        }
        region(ui, middle, |ui| match screen {
            Screen::Main => self.main_middle(ui),
            Screen::Run => self.run_view(ui),
            Screen::Review => self.review_view(ui),
        });
        if settings {
            region(ui, panel, |ui| self.settings_panel(ui));
        }

        let painter = ui.painter();
        let line = egui::Stroke::new(1.0, p.separator);
        painter.hline(whole.x_range(), below_bar - 0.5, line);
        if sidebar {
            painter.vline(sidebar_right - 0.5, whole.y_range(), line);
        }
        if settings {
            painter.vline(settings_left + 0.5, below_bar..=whole.bottom(), line);
        }
        if let (Screen::Run, Some(fraction)) = (screen, self.progress_line) {
            let left = if sidebar { sidebar_right } else { whole.left() };
            let done = egui::Rect::from_x_y_ranges(
                left..=left + (whole.right() - left) * fraction,
                below_bar - 2.0..=below_bar,
            );
            painter.rect_filled(done, 0.0, p.text);
        }

        // A click can open or close a review after the toolbar and the columns were laid out for
        // the screen before it. Redoing the pass keeps any frame from mixing the two, and has what
        // the new screen asks of its threads asked in the frame that made the change.
        if self.screen() != screen || self.sidebar_shown() != sidebar {
            ui.ctx()
                .request_discard("the screen changed while it was laid out");
        }
    }

    /// The app's icon and name above the sidebar, in the toolbar's height.
    fn sidebar_head(&mut self, ui: &egui::Ui, head: egui::Rect) {
        const ICON: f32 = 20.0;
        let p = Palette::of(ui);
        let icon = self.icon.get_or_insert_with(|| {
            // The 512 px icon is scaled down here: egui samples textures without mipmaps, so
            // drawing it at 20 px would alias.
            let image = image::load_from_memory_with_format(APP_ICON, image::ImageFormat::Png)
                .expect("the bundled icon decodes")
                .resize_exact(64, 64, image::imageops::FilterType::Lanczos3)
                .into_rgba8();
            let pixels = egui::ColorImage::from_rgba_unmultiplied([64, 64], image.as_raw());
            ui.ctx()
                .load_texture("app-icon", pixels, egui::TextureOptions::LINEAR)
        });
        let left = head.left() + 18.0;
        let picture = egui::Rect::from_min_size(
            egui::pos2(left, head.center().y - ICON / 2.0),
            egui::vec2(ICON, ICON),
        );
        egui::Image::new((icon.id(), picture.size())).paint_at(ui, picture);
        ui.painter().text(
            egui::pos2(picture.right() + 8.0, head.center().y),
            egui::Align2::LEFT_CENTER,
            "CloakFrame",
            style::medium(13.0),
            p.text,
        );
    }

    fn sidebar_shown(&self) -> bool {
        !self.review.as_ref().is_some_and(ReviewScreen::in_editor)
    }

    /// The screen's title, what it holds, and its actions at the right.
    fn toolbar(&mut self, ui: &mut egui::Ui, title_left: f32) {
        let p = *Palette::of(ui);
        let tr = Rc::clone(&self.tr);
        if let Some(screen) = &mut self.review {
            let act = screen.toolbar(ui, ui.max_rect(), title_left, &tr);
            self.screen_act(act);
            return;
        }
        let (title, detail) = match self.screen() {
            Screen::Main => {
                let detail = self
                    .found
                    .as_ref()
                    .filter(|f| !f.files.is_empty())
                    .map(|found| {
                        let count = |kind| found.files.iter().filter(|f| f.kind == kind).count();
                        tr.with(
                            "found-counts",
                            &[
                                ("photos", FluentValue::from(count(MediaKind::Image))),
                                ("videos", FluentValue::from(count(MediaKind::Video))),
                            ],
                        )
                    });
                (tr.get("new-task"), detail)
            }
            Screen::Run => self.run_heading(),
            Screen::Review => unreachable!("the review draws its own toolbar"),
        };
        let rect = ui.max_rect();
        let title_rect =
            egui::Rect::from_x_y_ranges(title_left + 20.0..=rect.right(), rect.y_range());
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(title_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.label(
                    egui::RichText::new(title)
                        .font(style::medium(14.0))
                        .color(p.text),
                );
                if let Some(detail) = detail {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(detail).color(p.secondary));
                }
            },
        );
        match self.screen() {
            Screen::Main => self.main_actions(ui, rect),
            Screen::Run => self.run_actions(ui, rect),
            Screen::Review => {}
        }
    }

    /// Add… once there are files, where the empty middle no longer offers it, and Remove All.
    fn main_actions(&mut self, ui: &mut egui::Ui, bar: egui::Rect) {
        let (add, clear) = (self.tr.get("add"), self.tr.get("clear-list"));
        let has_inputs = !self.inputs.is_empty();
        let has_files = self.found.as_ref().is_some_and(|f| !f.files.is_empty());
        let mut widths = Vec::new();
        if has_files {
            widths.push(style::button_width(ui, &add, false));
        }
        if has_inputs {
            widths.push(style::button_width(ui, &clear, false));
        }
        if widths.is_empty() {
            return;
        }
        let spacing = ui.spacing().item_spacing.x;
        let width = widths.iter().sum::<f32>() + spacing * (widths.len() - 1) as f32;
        let right = bar.right() - 14.0;
        let actions = egui::Rect::from_x_y_ranges(right - width..=right, bar.y_range());
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(actions)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                let (files, folder) = (
                    self.tr.get("add-menu-files"),
                    self.tr.get("add-menu-folder"),
                );
                if has_files {
                    let button = egui::Button::new(add)
                        .frame_when_inactive(false)
                        .min_size(egui::vec2(0.0, style::BUTTON_HEIGHT))
                        .corner_radius(style::BUTTON_RADIUS);
                    egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
                        if ui.button(files).clicked() {
                            self.command(Command::AddFiles);
                        }
                        if ui.button(folder).clicked() {
                            self.command(Command::AddFolder);
                        }
                    });
                }
                if has_inputs && style::plain(ui, clear).clicked() {
                    self.clear_inputs();
                }
            },
        );
    }

    /// The sidebar's list above, and the way to the settings at its foot.
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        const FOOT: f32 = 46.0;
        let whole = ui.max_rect();
        let (list, foot) = whole.split_top_bottom_at_y(whole.bottom() - FOOT);
        region(ui, list, |ui| self.sidebar_list(ui));
        region(ui, foot, |ui| {
            let label = self.tr.get("settings-title");
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    let item = style::sidebar_item(ui, false, &label, 30.0, |ui, p| {
                        let (icon, _) =
                            ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                        style::icon::settings(ui.painter(), icon, p.body);
                        ui.label(egui::RichText::new(&label).color(p.body));
                    });
                    if item.clicked() {
                        self.command(Command::Settings);
                    }
                });
        });
    }

    /// The new task or the run, then the reviews earlier runs left (plan 6, M6 scope ②).
    fn sidebar_list(&mut self, ui: &mut egui::Ui) {
        let p = *Palette::of(ui);
        let tr = &self.tr;
        let mut act = None;
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                let label = match &self.board {
                    Some(board) if board.is_over() => {
                        tr.get(result_title(board, &self.sessions, self.review.as_ref()))
                    }
                    Some(_) => tr.get("running"),
                    None => tr.get("new-task"),
                };
                let count = match &self.board {
                    Some(board) => Some(board.items.len()),
                    None => self.found.as_ref().map(|f| f.files.len()),
                }
                .filter(|&n| n > 0);
                let selected = self.review.is_none();
                let item = style::sidebar_item(ui, selected, &label, 30.0, |ui, p| {
                    ui.label(sidebar_text(&label, selected, p));
                    if let Some(count) = count {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(count.to_string())
                                    .size(12.0)
                                    .color(p.secondary),
                            );
                        });
                    }
                });
                if item.clicked() && self.review.is_some() {
                    act = Some(SidebarAct::Close);
                }
                if let Some(e) = &self.sessions.error {
                    ui.label(egui::RichText::new(e).size(12.0).color(p.needs_review));
                }
                if self.sessions.list.is_empty() {
                    return;
                }
                let title = tr.get("sessions-title");
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(&title)
                            .font(style::medium(11.0))
                            .color(p.secondary),
                    );
                });
                ui.add_space(4.0);
                let now = SystemTime::now();
                let open_id = self.review.as_ref().map(|r| r.review.id.clone());
                a11y::list(ui, &title, |ui| {
                    for saved in &self.sessions.list {
                        let row = SessionRow::of(saved, now, tr);
                        let selected = open_id.as_ref() == Some(&saved.id);
                        let response = a11y::item(ui, &row.described, |ui| {
                            style::sidebar_item(ui, selected, &row.described, 40.0, |ui, p| {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    ui.label(sidebar_text(&row.name, selected, p));
                                    if let Some(age) = &row.age {
                                        ui.label(
                                            egui::RichText::new(age).size(11.0).color(p.secondary),
                                        );
                                    }
                                });
                                if let Some(count) = row.count {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new(count.to_string())
                                                    .size(12.0)
                                                    .color(p.secondary),
                                            );
                                        },
                                    );
                                }
                            })
                        });
                        let response = response.on_hover_text(&row.detail);
                        if response.clicked() && row.readable && !selected {
                            act = Some(SidebarAct::Open(saved.id.clone()));
                        }
                        response.context_menu(|ui| {
                            if ui.button(tr.get("session-discard")).clicked() {
                                act = Some(SidebarAct::Discard(saved.id.clone()));
                            }
                        });
                    }
                });
            });
        match act {
            None => {}
            Some(SidebarAct::Close) => {
                self.review = None;
                self.sessions_read = false;
            }
            Some(SidebarAct::Open(id)) => {
                let ctx = ui.ctx().clone();
                self.open_review(id, None, &ctx);
            }
            Some(SidebarAct::Discard(id)) => self.discarding = Some(id),
        }
    }

    /// A run, or a review's renders, still working.
    fn working(&self) -> bool {
        self.running.is_some()
            || self
                .review
                .as_ref()
                .is_some_and(|r| r.review.is_rendering())
    }

    /// Keeps the window open while a run or a review's renders work, until the user agrees to
    /// cancel them.
    fn watch_close(&mut self, ctx: &egui::Context) {
        let working = self.working();
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
                if let Some(screen) = &self.review {
                    screen.review.cancel();
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
            Command::Settings => self.open_settings(None, ctx),
            Command::Models => self.open_settings(Some(SettingsTab::Models), ctx),
            Command::About => self.about_open = true,
            Command::CheckUpdates => self.updates.check(true, ctx),
            Command::CheckUpdatesOnStart(on) => {
                self.settings.check_updates = on;
                self.save_settings();
                if !on {
                    self.updates.forget_startup_check();
                }
            }
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::Theme(theme) => {
                self.settings.theme = theme;
                self.save_settings();
                // This pass began in the old colours, and a menu choice repaints nothing else.
                ctx.set_theme(theme_preference(theme));
                ctx.request_repaint();
            }
            Command::Language(choice) => {
                self.set_language(choice);
                self.save_settings();
            }
        }
    }

    fn open_settings(&mut self, tab: Option<SettingsTab>, ctx: &egui::Context) {
        self.dialog_open = true;
        if let Some(tab) = tab {
            self.settings_tab = tab;
        }
        self.models.check(ctx);
        self.gpu_pack.check();
    }

    /// The files to process, or where to drop them; under them, what is left out and why Start
    /// waits.
    fn main_middle(&mut self, ui: &mut egui::Ui) {
        let p = *Palette::of(ui);
        let ctx = ui.ctx().clone();
        let lines = self.middle_lines(&p);
        if !lines.is_empty() {
            egui::Panel::bottom("middle-lines")
                .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                    left: 24,
                    right: 24,
                    top: 12,
                    bottom: 16,
                }))
                .show_separator_line(false)
                .show(ui, |ui| {
                    let top = ui.max_rect().top() - 12.0;
                    ui.painter().hline(
                        ui.max_rect().x_range(),
                        top,
                        egui::Stroke::new(1.0, p.faint_line),
                    );
                    for (text, hover) in lines {
                        let response = ui.label(text);
                        if let Some(hover) = hover {
                            response.on_hover_text(hover);
                        }
                    }
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let working = self.working();
                self.updates.bar(ui, &self.tr, working);
                for notice in &self.notices {
                    ui.colored_label(p.needs_review, notice);
                }
                self.models_needed(ui);
                if self.found.as_ref().is_some_and(|f| !f.files.is_empty()) {
                    self.file_grid(ui, &ctx);
                } else {
                    self.drop_here(ui, &ctx);
                }
            });
    }

    /// The lines under the files, each with the details its hover shows.
    fn middle_lines(&self, p: &Palette) -> Vec<(egui::RichText, Option<String>)> {
        let tr = &self.tr;
        let mut lines = Vec::new();
        if let Some(found) = &self.found {
            let mut parts = Vec::new();
            let mut count = 0;
            for (extension, n) in &found.skipped {
                count += n;
                let n = FluentValue::from(*n);
                parts.push(if extension.is_empty() {
                    tr.with("skipped-part-no-extension", &[("count", n)])
                } else {
                    tr.with(
                        "skipped-part-type",
                        &[
                            ("count", n),
                            ("extension", FluentValue::from(extension.as_str())),
                        ],
                    )
                });
            }
            if !found.issues.is_empty() {
                count += found.issues.len();
                parts.push(tr.with(
                    "skipped-part-unreadable",
                    &[("count", FluentValue::from(found.issues.len()))],
                ));
            }
            if count > 0 {
                let text = tr.with(
                    "skipped-summary",
                    &[
                        ("count", FluentValue::from(count)),
                        (
                            "parts",
                            FluentValue::from(parts.join(&tr.get("list-separator"))),
                        ),
                    ],
                );
                let unreadable: Vec<String> = found
                    .issues
                    .iter()
                    .map(|issue| {
                        let path = issue.path.display().to_string();
                        tr.with("unreadable", &[("path", FluentValue::from(path))])
                    })
                    .collect();
                let hover = (!unreadable.is_empty()).then(|| unreadable.join("\n"));
                lines.push((egui::RichText::new(text).color(p.secondary), hover));
            }
        }
        if let Some(found) = self.found.as_ref().filter(|f| f.videos_left_out > 0) {
            let text = tr.with(
                "videos-left-out",
                &[("count", FluentValue::from(found.videos_left_out))],
            );
            lines.push((egui::RichText::new(text).color(p.secondary), None));
        }
        let has_files = self.found.as_ref().is_some_and(|f| !f.files.is_empty());
        // Without faces or plates the panel says so beside the boxes, and a missing model has its
        // band above the files.
        let shown_beside = |why: &&str| matches!(*why, "models-need-one" | "start-needs-models");
        if let Some(why) = self
            .start_blocker()
            .filter(|why| has_files && !shown_beside(why))
        {
            lines.push((egui::RichText::new(tr.get(why)).color(p.text), None));
        }
        lines
    }

    /// The models this run would need and does not have, with their terms and a way to get
    /// them, before Start rather than as a run that fails.
    fn models_needed(&mut self, ui: &mut egui::Ui) {
        let missing = self.missing_models();
        // The dialog lists every model itself, over this.
        if missing.is_empty() || self.dialog_open {
            return;
        }
        let p = *Palette::of(ui);
        let tr = self.tr.clone();
        let band = egui::Frame::NONE
            .fill(p.sidebar)
            .inner_margin(egui::Margin::symmetric(24, 14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                style::group_title(ui, &tr.get("models-needed"));
                ui.add_space(4.0);
                self.models.rows_of(ui, &tr, &missing);
            });
        ui.painter().hline(
            band.response.rect.x_range(),
            band.response.rect.bottom(),
            egui::Stroke::new(1.0, p.separator),
        );
    }

    fn missing_models(&self) -> Vec<ModelId> {
        run::RunOptions::of(&self.settings)
            .required_models()
            .into_iter()
            .filter(|model| self.models.is_missing(*model))
            .collect()
    }

    fn drop_here(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = *Palette::of(ui);
        let hovering = ctx.input(|i| i.raw.hovered_files.len());
        let hint = if hovering > 0 {
            self.tr
                .with("drop-release", &[("count", FluentValue::from(hovering))])
        } else {
            self.tr.get("drop-hint")
        };
        let rect = ui.available_rect_before_wrap();
        if hovering > 0 {
            ui.painter().rect_stroke(
                rect.shrink(12.0),
                style::BUTTON_RADIUS,
                egui::Stroke::new(1.5, p.border),
                egui::StrokeKind::Inside,
            );
        }
        let lines = if self.scanning.is_some() { 3.0 } else { 2.0 };
        let height = lines * 26.0;
        let block = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), height));
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(block)
                .layout(egui::Layout::top_down(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                ui.label(egui::RichText::new(hint).size(15.0).color(p.body));
                ui.horizontal(|ui| {
                    let (or, files, folder) = (
                        self.tr.get("drop-or"),
                        self.tr.get("choose-files"),
                        self.tr.get("choose-input-folder"),
                    );
                    const DOT: &str = "·";
                    let spacing = 4.0;
                    let width = [or.as_str(), files.as_str(), DOT, folder.as_str()]
                        .map(|text| style::text_width(ui, text))
                        .iter()
                        .sum::<f32>()
                        + 3.0 * spacing;
                    ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                    ui.spacing_mut().item_spacing.x = spacing;
                    ui.label(egui::RichText::new(or).color(p.secondary));
                    if style::link(ui, &files).clicked() {
                        self.command(Command::AddFiles);
                    }
                    ui.label(egui::RichText::new(DOT).color(p.secondary));
                    if style::link(ui, &folder).clicked() {
                        self.command(Command::AddFolder);
                    }
                });
                if self.scanning.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(egui::RichText::new(self.tr.get("scanning")).color(p.secondary));
                    });
                }
            },
        );
    }

    /// The thumbnails of what was found, which the grid and the run's rows share.
    fn open_thumbnails(&mut self, ctx: &egui::Context) {
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
    }

    fn file_grid(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = *Palette::of(ui);
        self.open_thumbnails(ctx);
        let Some(found) = &self.found else {
            return;
        };
        let thumbnails = &mut self.thumbnails;
        let tr = &self.tr;
        let list = tr.get("found-list");
        let mut remove = None;
        egui::ScrollArea::vertical()
            .id_salt("files")
            .auto_shrink(false)
            .show(ui, |ui| {
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(24, 22))
                    .show(ui, |ui| {
                        let (gap_x, gap_y) = (14.0, 18.0);
                        let width = ui.available_width();
                        let columns = ((width + gap_x) / (style::GRID_MIN_CELL + gap_x))
                            .floor()
                            .max(1.0);
                        let cell = (width - gap_x * (columns - 1.0)) / columns;
                        let picture_height = cell * 3.0 / 4.0;
                        ui.spacing_mut().item_spacing = egui::vec2(gap_x, gap_y);
                        let columns_n = columns as usize;
                        let rows = found.files.len().div_ceil(columns_n);
                        let cell_height = picture_height + 24.0;
                        let height =
                            rows as f32 * cell_height + rows.saturating_sub(1) as f32 * gap_y;
                        a11y::list(ui, &list, |ui| {
                            let (grid, _) = ui.allocate_exact_size(
                                egui::vec2(width, height),
                                egui::Sense::hover(),
                            );
                            for (i, file) in found.files.iter().enumerate() {
                                let (row, column) = (i / columns_n, i % columns_n);
                                let rect = egui::Rect::from_min_size(
                                    grid.min
                                        + egui::vec2(
                                            column as f32 * (cell + gap_x),
                                            row as f32 * (cell_height + gap_y),
                                        ),
                                    egui::vec2(cell, cell_height),
                                );
                                let name = file.relative.display().to_string();
                                let kind = match file.kind {
                                    MediaKind::Image => "found-photo",
                                    MediaKind::Video => "found-video",
                                };
                                let described =
                                    tr.with(kind, &[("name", FluentValue::from(name.as_str()))]);
                                let response = ui.interact(
                                    rect,
                                    ui.make_persistent_id(("file", i)),
                                    egui::Sense::hover(),
                                );
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::Role::ListItem,
                                        true,
                                        &described,
                                    )
                                });
                                let picture = egui::Rect::from_min_size(
                                    rect.min,
                                    egui::vec2(cell, picture_height),
                                );
                                let out = remove_button(ui, i, picture, &name, tr);
                                if out.gained_focus() {
                                    out.scroll_to_me(None);
                                }
                                if out.clicked() {
                                    remove = Some(file.path.clone());
                                }
                                if !ui.is_rect_visible(rect) {
                                    continue;
                                }
                                let shown = thumbnails
                                    .as_mut()
                                    .and_then(|t| t.get(&file.path, file.kind).flatten());
                                match shown {
                                    Some(texture) => {
                                        egui::Image::new(texture)
                                            .uv(style::cover(texture.size_vec2(), picture.size()))
                                            .corner_radius(style::THUMBNAIL_RADIUS)
                                            .paint_at(ui, picture);
                                    }
                                    None => {
                                        ui.painter().rect_filled(
                                            picture,
                                            style::THUMBNAIL_RADIUS,
                                            p.separator,
                                        );
                                    }
                                }
                                if let Some(seconds) =
                                    thumbnails.as_ref().and_then(|t| t.seconds(&file.path))
                                {
                                    duration_badge(ui, picture, seconds);
                                }
                                let mut job = egui::text::LayoutJob::simple_singleline(
                                    name,
                                    egui::FontId::proportional(12.0),
                                    p.text,
                                );
                                job.wrap = egui::text::TextWrapping::truncate_at_width(cell);
                                let galley = ui.painter().layout_job(job);
                                ui.painter().galley(
                                    egui::pos2(rect.min.x, picture.max.y + 6.0),
                                    galley,
                                    p.text,
                                );
                                if ui.rect_contains_pointer(rect) || out.has_focus() {
                                    paint_remove_button(ui, &out);
                                }
                            }
                        });
                    });
            });
        if let Some(path) = remove {
            self.remove_input(&path);
        }
    }

    /// What to hide and how, where to save and whether to review first, and Start at the foot,
    /// where the review has its approve button.
    fn settings_panel(&mut self, ui: &mut egui::Ui) {
        let before = self.settings.clone();
        let tr = self.tr.clone();
        let p = *Palette::of(ui);
        let whole = ui.max_rect();
        let (top, footer) = whole.split_top_bottom_at_y(whole.bottom() - PANEL_FOOTER_HEIGHT);
        let mut sample = (0.0, 0.0);
        let scroll = region(ui, top, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("settings")
                .auto_shrink(false)
                .show(ui, |ui| {
                    style::group(ui, |ui| {
                        style::group_title(ui, &tr.get("what"));
                        ui.add(style::Check::new(&mut self.settings.faces, tr.get("faces")));
                        ui.add(style::Check::new(
                            &mut self.settings.plates,
                            tr.get("plates"),
                        ));
                        if !self.settings.faces && !self.settings.plates {
                            ui.colored_label(p.needs_review, tr.get("models-need-one"));
                        }
                    });
                    style::group(ui, |ui| {
                        style::group_title(ui, &tr.get("how"));
                        let methods = [
                            (Method::Fill, tr.get("method-fill")),
                            (Method::Mosaic, tr.get("method-mosaic")),
                            (Method::Blur, tr.get("method-blur")),
                        ];
                        ui.push_id("method", |ui| {
                            style::segmented(ui, &mut self.settings.method, &methods);
                        });
                        self.shape(ui, &tr);
                        if self.settings.method == Method::Mosaic {
                            self.strength(ui, &tr);
                        }
                        sample = self.sample_preview(ui);
                    });
                    style::group(ui, |ui| {
                        style::group_title(ui, &tr.get("output-group"));
                        self.output_button(ui, &tr);
                        ui.add(style::Check::new(
                            &mut self.settings.review,
                            tr.get("review-first"),
                        ));
                    });
                })
        });
        self.fit_sample(ui.ctx(), &scroll, sample);
        region(ui, footer, |ui| {
            ui.painter().hline(
                footer.x_range(),
                footer.top(),
                egui::Stroke::new(1.0, p.faint_line),
            );
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(20, 16))
                .show(ui, |ui| {
                    // A button lays its words out by the layout it is in: top-down puts them left.
                    ui.horizontal(|ui| {
                        let start = tr.get("start");
                        let blocker = self.start_blocker();
                        let size = egui::vec2(ui.available_width(), 32.0);
                        let response = style::primary_sized(ui, &start, blocker.is_none(), size);
                        if response.clicked() {
                            self.start_run(ui.ctx());
                        }
                        if let Some(why) = blocker {
                            response.on_disabled_hover_text(tr.get(why));
                        }
                    });
                });
        });
        if self.settings != before {
            self.save_settings();
        }
    }

    fn shape(&mut self, ui: &mut egui::Ui, tr: &Translator) {
        let label_width = how_label_width(ui, tr);
        style::field(ui, label_width, &tr.get("shape"), |ui| {
            let shapes = [
                (Shape::Rectangle, tr.get("shape-rectangle")),
                (Shape::Ellipse, tr.get("shape-ellipse")),
            ];
            ui.push_id("shape", |ui| {
                style::segmented(ui, &mut self.settings.shape, &shapes);
            });
        });
        style::field(ui, label_width, &tr.get("coverage"), |ui| {
            let coverages = [
                (Coverage::Narrow, tr.get("coverage-narrow")),
                (Coverage::Standard, tr.get("coverage-standard")),
                (Coverage::Wide, tr.get("coverage-wide")),
            ];
            ui.push_id("coverage", |ui| {
                style::segmented(ui, &mut self.settings.coverage, &coverages);
            });
        })
        .on_hover_text(tr.get("coverage-hint"));
    }

    fn strength(&mut self, ui: &mut egui::Ui, tr: &Translator) {
        use cloakframe_core::mask::MosaicStrength;
        let label = tr.get("strength");
        let label_width = how_label_width(ui, tr);
        style::field(ui, label_width, &label, |ui| {
            // The slider is shorter than the label's cell; a line of the cell's height centres
            // both on one line.
            let line = egui::vec2(ui.available_width(), style::BUTTON_HEIGHT);
            let centered = egui::Layout::left_to_right(egui::Align::Center);
            ui.allocate_ui_with_layout(line, centered, |ui| {
                ui.set_min_height(style::BUTTON_HEIGHT);
                let width = ui.available_width() - 16.0 - ui.spacing().item_spacing.x;
                let value = &mut self.settings.mosaic_strength;
                let slider =
                    style::slider(ui, value, MosaicStrength::MIN..=MosaicStrength::MAX, width);
                let shown = *value;
                slider.widget_info(|| egui::WidgetInfo::slider(true, f64::from(shown), &label));
                ui.label(shown.to_string());
            });
        });
    }

    /// What a run with the settings as they are would make of the sample photo, cut to the
    /// panel's width around the face and, where the panel is short, to the room it has. Returns
    /// the height drawn and the height it would have with room to spare.
    fn sample_preview(&mut self, ui: &mut egui::Ui) -> (f32, f32) {
        let options = run::RunOptions::of(&self.settings);
        let texture = self.sample.texture(ui.ctx(), &options);
        let width = ui.available_width();
        let natural = (width * SAMPLE_ASPECT).round();
        let height = self.sample_room.map_or(natural, |room| {
            room.min(natural).max(SAMPLE_MIN_HEIGHT).floor()
        });
        let size = egui::vec2(width, height);
        let shown = size.y / size.x;
        let top = 0.1 * (1.0 - shown);
        let image = egui::Image::new((texture.id(), size))
            .uv(egui::Rect::from_min_max(
                egui::pos2(0.0, top),
                egui::pos2(1.0, top + shown),
            ))
            .corner_radius(style::THUMBNAIL_RADIUS)
            .alt_text(self.tr.get("sample-alt"));
        ui.add(image);
        (height, natural)
    }

    /// Gives the sample photo the panel's spare height, or takes from it what the panel runs
    /// over, so the output folder stays in view; the frame is redone when the photo's height
    /// changes, so a change of the window or the rows never shows a frame that runs over.
    fn fit_sample<R>(
        &mut self,
        ctx: &egui::Context,
        scroll: &egui::scroll_area::ScrollAreaOutput<R>,
        (drawn, natural): (f32, f32),
    ) {
        let over = scroll.content_size.y - scroll.inner_rect.height();
        let room = drawn - over;
        self.sample_room = Some(room);
        let next = room.min(natural).max(SAMPLE_MIN_HEIGHT).floor();
        if (next - drawn).abs() >= 1.0 {
            ctx.request_discard("sample photo fitted to the settings panel");
        }
    }

    fn output_button(&mut self, ui: &mut egui::Ui, tr: &Translator) {
        let p = *Palette::of(ui);
        let (shown, change) = match self.output() {
            Some(folder) => (
                folder.file_name().map_or_else(
                    || folder.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                ),
                Some(tr.get("change-folder")),
            ),
            None => (tr.get("choose-folder"), None),
        };
        let mut button = egui::Button::new(shown)
            .min_size(egui::vec2(ui.available_width(), style::BUTTON_HEIGHT))
            .corner_radius(style::BUTTON_RADIUS);
        if let Some(change) = change {
            button = button.right_text(egui::RichText::new(change).color(p.secondary));
        }
        let mut response = ui.add(button);
        if let Some(folder) = self.output() {
            response = response.on_hover_text(folder.display().to_string());
        }
        if response.clicked()
            && let Some(folder) = rfd::FileDialog::new().pick_folder()
        {
            self.settings.output = Some(folder);
            if !self.inputs.is_empty() {
                let ctx = ui.ctx().clone();
                self.rescan(&ctx);
            }
        }
    }

    /// The settings, in a dialog in the middle of the window over a darkened backdrop, one tab at
    /// a time. Its title and its Done line stay put while the tab between them scrolls.
    fn settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.dialog_open {
            return;
        }
        let before = self.settings.clone();
        let tr = self.tr.clone();
        let (p, backdrop) = if ctx.theme() == egui::Theme::Dark {
            (style::DARK, egui::Color32::from_black_alpha(128))
        } else {
            (
                style::LIGHT,
                egui::Color32::from_rgba_unmultiplied(24, 24, 27, 71),
            )
        };
        let id = egui::Id::unique("settings");
        let frame = egui::Frame::NONE
            .fill(p.window)
            .stroke(egui::Stroke::new(1.0, p.border))
            .corner_radius(DIALOG_RADIUS)
            .shadow(egui::Shadow {
                offset: [0, 12],
                blur: 32,
                spread: 0,
                color: egui::Color32::from_black_alpha(46),
            });
        let height =
            (ctx.content_rect().height() - 2.0 * DIALOG_MARGIN).clamp(240.0, DIALOG_MAX_HEIGHT);
        let line = egui::Stroke::new(1.0, p.faint_line);
        let mut done = false;
        let modal = egui::Modal::new(id)
            .frame(frame)
            .backdrop_color(backdrop)
            .show(ctx, |ui| {
                ui.set_width(DIALOG_WIDTH);
                let top = ui.cursor().top();
                ui.spacing_mut().item_spacing.y = 0.0;
                let label = tr.get("settings-title");
                ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
                    node.set_role(egui::accesskit::Role::Dialog);
                    node.set_label(label.as_str());
                });
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(24, 12))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&label).font(style::medium(14.0)));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let close = tr.get("close");
                                    if style::icon_button(ui, &close, style::icon::close).clicked()
                                    {
                                        done = true;
                                    }
                                },
                            );
                        });
                    });
                ui.painter()
                    .hline(ui.max_rect().x_range(), ui.min_rect().bottom(), line);
                // An area's ui has no bottom to lay out up from, so the part under the title is
                // given its own rect down to the dialog's height.
                let rest = egui::Rect::from_x_y_ranges(
                    ui.max_rect().x_range(),
                    ui.cursor().top()..=top + height,
                );
                ui.advance_cursor_after_rect(rest);
                let builder = egui::UiBuilder::new()
                    .max_rect(rest)
                    .layout(egui::Layout::bottom_up(egui::Align::Min));
                ui.scope_builder(builder, |ui| {
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(24, 14))
                        .show(ui, |ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), style::BUTTON_HEIGHT),
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if style::primary(ui, &tr.get("done"), true).clicked() {
                                        done = true;
                                    }
                                    ui.add_space(16.0);
                                    if self.settings_tab != SettingsTab::Models {
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(
                                                        tr.get("settings-next-run"),
                                                    )
                                                    .size(12.0)
                                                    .color(p.secondary),
                                                );
                                            },
                                        );
                                    }
                                },
                            );
                        });
                    ui.painter()
                        .hline(ui.max_rect().x_range(), ui.min_rect().top(), line);
                    let middle = ui.available_rect_before_wrap();
                    let (tabs, pane) =
                        middle.split_left_right_at_x(middle.left() + DIALOG_TABS_WIDTH);
                    ui.painter().rect_filled(tabs, 0.0, p.sidebar);
                    ui.painter().vline(tabs.right(), tabs.y_range(), line);
                    region(ui, tabs, |ui| self.settings_tabs(ui, &tr));
                    region(ui, pane, |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        egui::ScrollArea::vertical()
                            .id_salt(("settings", self.settings_tab))
                            .auto_shrink(false)
                            .show(ui, |ui| {
                                egui::Frame::NONE
                                    .inner_margin(egui::Margin {
                                        left: 24,
                                        right: 24,
                                        top: 16,
                                        bottom: 20,
                                    })
                                    .show(ui, |ui| match self.settings_tab {
                                        SettingsTab::General => self.general_settings(ui, &tr),
                                        SettingsTab::Detection => self.detection_settings(ui, &tr),
                                        SettingsTab::Output => self.output_settings(ui, &tr),
                                        SettingsTab::Models => self.model_settings(ui, &tr),
                                    });
                            });
                    });
                });
            });
        if done || modal.should_close() {
            self.dialog_open = false;
        }
        if self.settings != before {
            self.save_settings();
        }
    }

    fn settings_tabs(&mut self, ui: &mut egui::Ui, tr: &Translator) {
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(10, 10))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                for (tab, key) in [
                    (SettingsTab::General, "settings-tab-general"),
                    (SettingsTab::Detection, "settings-tab-detection"),
                    (SettingsTab::Output, "settings-tab-output"),
                    (SettingsTab::Models, "models-title"),
                ] {
                    let label = tr.get(key);
                    let selected = self.settings_tab == tab;
                    let item = style::sidebar_item(ui, selected, &label, 30.0, |ui, p| {
                        ui.label(sidebar_text(&label, selected, p));
                    });
                    if item.clicked() {
                        self.settings_tab = tab;
                    }
                }
            });
    }

    fn general_settings(&mut self, ui: &mut egui::Ui, tr: &Translator) {
        dialog_field(ui, &tr.get("language"), |ui| {
            let name = |language: Option<Language>| {
                language.map_or_else(|| tr.get("language-system"), |l| l.name().into())
            };
            let mut choice = self.settings.language;
            style::select(ui, "language", 220.0, name(choice), |ui| {
                for language in std::iter::once(None).chain(Language::ALL.map(Some)) {
                    ui.selectable_value(&mut choice, language, name(language));
                }
            })
            .response
            .widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::ComboBox, true, tr.get("language"))
            });
            if choice != self.settings.language {
                self.set_language(choice);
                ui.ctx().request_repaint();
            }
        });
        dialog_field(ui, &tr.get("theme"), |ui| {
            let themes = [
                (Theme::System, tr.get("theme-system")),
                (Theme::Light, tr.get("theme-light")),
                (Theme::Dark, tr.get("theme-dark")),
            ];
            let changed = ui
                .push_id("theme", |ui| {
                    style::segmented(ui, &mut self.settings.theme, &themes)
                })
                .inner;
            if changed {
                ui.ctx().request_repaint();
            }
        });
        ui.add_space(6.0);
        ui.add(style::Check::new(
            &mut self.settings.advanced.gpu,
            tr.get("gpu"),
        ));
        dialog_hint(ui, tr.get("gpu-hint"));
    }

    fn detection_settings(&mut self, ui: &mut egui::Ui, tr: &Translator) {
        let faces = self.settings.faces;
        let advanced = &mut self.settings.advanced;
        dialog_field(ui, &tr.get("face-model"), |ui| {
            ui.add_enabled_ui(faces, |ui| {
                let models = [
                    (FaceModel::Accurate, tr.get("face-model-accurate")),
                    (FaceModel::Fast, tr.get("face-model-fast")),
                ];
                ui.push_id("face-model", |ui| {
                    style::segmented(ui, &mut advanced.face_model, &models);
                });
            });
        });
        dialog_field(ui, &tr.get("sensitivity"), |ui| {
            advanced.sensitivity = advanced.sensitivity.clamp(1, 5);
            ui.horizontal(|ui| {
                let width = ui.available_width() - 90.0;
                let slider = style::slider(ui, &mut advanced.sensitivity, 1..=5, width)
                    .on_hover_text(tr.with(
                        "sensitivity-hint",
                        &[(
                            "score",
                            FluentValue::from(format!("{:.2}", advanced.score())),
                        )],
                    ));
                let level = tr.get(SENSITIVITY_LEVELS[usize::from(advanced.sensitivity) - 1]);
                slider.widget_info(|| {
                    let mut info = egui::WidgetInfo::slider(
                        true,
                        f64::from(advanced.sensitivity),
                        tr.get("sensitivity"),
                    );
                    info.current_text_value = Some(level.clone());
                    info
                });
                ui.label(level);
            });
        });
        ui.add_space(6.0);
        ui.add(style::Check::new(&mut advanced.tiles, tr.get("tiles")));
        dialog_hint(ui, tr.get("tiles-hint"));
    }

    fn output_settings(&mut self, ui: &mut egui::Ui, tr: &Translator) {
        let advanced = &mut self.settings.advanced;
        style::group_title(ui, &tr.get("settings-photos"));
        ui.add_space(2.0);
        ui.add(style::Check::new(
            &mut advanced.keep_metadata,
            tr.get("keep-metadata"),
        ))
        .on_hover_text(tr.get("keep-metadata-hint"));
        ui.horizontal(|ui| {
            ui.add_space(DIALOG_HINT_INDENT);
            ui.add_enabled(
                advanced.keep_metadata,
                style::Check::new(&mut advanced.keep_gps, tr.get("keep-gps")),
            );
        });
        dialog_section(ui, &tr.get("settings-videos"));
        let quality = |q: VideoQuality| match q {
            VideoQuality::High => "video-quality-high",
            VideoQuality::Balanced => "video-quality-balanced",
            VideoQuality::SpaceSaver => "video-quality-space-saver",
        };
        dialog_field(ui, &tr.get("video-quality"), |ui| {
            style::select(
                ui,
                "video-quality",
                220.0,
                tr.get(quality(advanced.video_quality)),
                |ui| {
                    for q in [
                        VideoQuality::High,
                        VideoQuality::Balanced,
                        VideoQuality::SpaceSaver,
                    ] {
                        ui.selectable_value(&mut advanced.video_quality, q, tr.get(quality(q)));
                    }
                },
            )
            .response
            .on_hover_text(tr.get("video-quality-hint"))
            .widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::ComboBox, true, tr.get("video-quality"))
            });
        });
        let codec = |c: VideoCodec| match c {
            VideoCodec::H264 => "video-codec-h264",
            VideoCodec::Hevc => "video-codec-hevc",
        };
        dialog_field(ui, &tr.get("video-codec"), |ui| {
            style::select(
                ui,
                "video-codec",
                220.0,
                tr.get(codec(advanced.video_codec)),
                |ui| {
                    for c in [VideoCodec::H264, VideoCodec::Hevc] {
                        ui.selectable_value(&mut advanced.video_codec, c, tr.get(codec(c)));
                    }
                },
            )
            .response
            .on_hover_text(tr.get("video-codec-hint"))
            .widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::ComboBox, true, tr.get("video-codec"))
            });
        });
        ui.add_space(6.0);
        ui.add(style::Check::new(
            &mut advanced.remove_audio,
            tr.get("remove-audio"),
        ));
        dialog_hint(ui, tr.get("remove-audio-hint"));
    }

    fn model_settings(&mut self, ui: &mut egui::Ui, tr: &Translator) {
        self.models.rows(ui, tr);
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            dialog_section(ui, &tr.get("gpu-pack-title"));
            let restart = crate::run::loaded_accelerator()
                .is_some_and(|a| a != Some(cloakframe_engine::detect::runtime::Backend::Cuda));
            self.gpu_pack.rows(ui, tr, restart);
        }
    }

    /// The message saying why the run cannot start yet.
    pub fn start_blocker(&self) -> Option<&'static str> {
        if self.review.is_some() {
            Some("start-during-review")
        } else if self.board.is_some() {
            Some("running")
        } else if !self.settings.faces && !self.settings.plates {
            Some("models-need-one")
        } else if self.scanning.is_some() {
            Some("start-scanning")
        } else if self.found.as_ref().is_none_or(|f| f.files.is_empty()) {
            Some("start-needs-files")
        } else if !self.missing_models().is_empty() {
            Some("start-needs-models")
        } else if self.output().is_none() {
            Some("start-needs-output")
        } else if self.tools.ort_library.is_none() {
            Some("start-needs-runtime")
        } else {
            None
        }
    }

    fn start_run(&mut self, ctx: &egui::Context) {
        let (Some(found), Some(output)) = (&self.found, self.output().cloned()) else {
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
                self.board = Some(Board::new(Instant::now(), output));
            }
            Err(e) => self.notices.push(e.to_string()),
        }
    }

    /// Opens session `id` for review, at `item` if given; a session another run or review
    /// holds is refused.
    pub fn open_review(&mut self, id: SessionId, item: Option<ItemId>, ctx: &egui::Context) {
        match Review::open(id, &self.folders, &self.tools, &self.settings, ctx) {
            Ok(review) => {
                let mut screen = ReviewScreen::new(review);
                if let Some(item) = item {
                    screen.open_item(item, ctx);
                }
                self.review = Some(screen);
            }
            Err(OpenError::Session(SessionError::InUse)) => {
                self.notices.push(self.tr.get("review-in-use"));
            }
            Err(e) => self.notices.push(e.to_string()),
        }
    }

    /// Waits for the review's renders, for tests.
    pub fn wait_for_review(&mut self) {
        if let Some(screen) = &mut self.review {
            screen.review.wait();
        }
    }

    fn review_view(&mut self, ui: &mut egui::Ui) {
        let Some(screen) = &mut self.review else {
            return;
        };
        let video = self.tools.ffmpeg.clone().zip(self.tools.ffprobe.clone());
        let act = screen.ui(ui, &self.tr, video);
        self.screen_act(act);
    }

    fn screen_act(&mut self, act: Option<ScreenAct>) {
        match act {
            Some(ScreenAct::Discard) => {
                self.discarding = self.review.as_ref().map(|s| s.review.id.clone());
            }
            Some(ScreenAct::Show(path, open)) => self.show(&path, open),
            None => {}
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

    /// Waits until the run in progress has named its items, for tests that act on one.
    pub fn wait_for_run_items(&mut self) {
        let (Some(running), Some(board)) = (&self.running, &mut self.board) else {
            return;
        };
        while board.items.is_empty() && !board.is_over() {
            match running.wait() {
                Some(message) => board.take(message),
                None => break,
            }
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

    /// The run's items as they are worked on, then what became of each (plan 6, M6 scope ③).
    fn run_view(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.open_thumbnails(&ctx);
        let Self {
            board: Some(board),
            thumbnails,
            found,
            tr,
            ..
        } = self
        else {
            return;
        };
        if !board.is_over() {
            ctx.request_repaint_after(PROGRESS_REDRAW);
        }
        let mut act = run_summary(ui, board, tr);
        let details = egui::Id::unique("run-details");
        // The details' height as the last frame drew them, folded or not.
        let details_height = details.with("height");
        let reserve = ctx
            .data(|d| d.get_temp::<f32>(details_height))
            .unwrap_or(0.0);
        let progress = board
            .progress
            .as_ref()
            .map(|p| p.snapshot())
            .unwrap_or_default();
        let session = board.finished.as_ref().and_then(|f| f.session.clone());
        egui::ScrollArea::vertical()
            .id_salt("run-items")
            .auto_shrink([false, false])
            .max_height((ui.available_height() - reserve).max(ROW_HEIGHT))
            .show(ui, |ui| {
                a11y::list(ui, &tr.get("run-list"), |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (item, input) in &board.items {
                        let row = RunRow::of(board, *item, input, &progress, session.is_some(), tr);
                        let next = egui::Rect::from_min_size(
                            ui.cursor().min,
                            egui::vec2(ui.available_width(), ROW_HEIGHT),
                        );
                        let thumbnail = if ui.is_rect_visible(next) {
                            let kind = found
                                .as_ref()
                                .and_then(|f| f.files.iter().find(|f| &f.path == input))
                                .map_or(MediaKind::Image, |f| f.kind);
                            thumbnails
                                .as_mut()
                                .and_then(|t| t.get(input, kind).flatten())
                        } else {
                            None
                        };
                        let chosen = ui.scope(|ui| {
                            a11y::mark_item(ui, &row.described);
                            run_row(ui, &row, thumbnail)
                        });
                        match chosen.inner {
                            None => {}
                            Some(RowAct::Cancel) => act = Act::CancelItem(*item),
                            Some(RowAct::Show(path, open)) => act = Act::Show(path, open),
                            Some(RowAct::Review) => {
                                if let Some(session) = &session {
                                    act = Act::Review(session.clone(), Some(*item));
                                }
                            }
                        }
                    }
                });
            });
        let list_bottom = ui.min_rect().bottom();
        run_details(ui, board, tr, details);
        let needed = ui.min_rect().bottom() - list_bottom;
        if (needed - reserve).abs() >= 0.5 {
            ctx.data_mut(|d| d.insert_temp(details_height, needed));
            ctx.request_discard("run details measured");
        }
        self.run_act(act, &ctx);
    }

    fn run_act(&mut self, act: Act, ctx: &egui::Context) {
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
            Act::Review(id, item) => self.open_review(id, item, ctx),
        }
    }

    /// The run's title with how far it is, or once it is over, when it ended and where it saved.
    fn run_heading(&mut self) -> (String, Option<String>) {
        self.progress_line = None;
        let tr = &self.tr;
        let Some(board) = &mut self.board else {
            return (tr.get("running"), None);
        };
        if board.is_over() {
            let folder = board.output.file_name().map_or_else(
                || board.output.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            let detail = board.ended.map(|at| {
                tr.with(
                    "run-ended",
                    &[
                        (
                            "when",
                            FluentValue::from(clock::when(at, SystemTime::now(), tr)),
                        ),
                        ("folder", FluentValue::from(folder)),
                    ],
                )
            });
            return (
                tr.get(result_title(board, &self.sessions, self.review.as_ref())),
                detail,
            );
        }
        if board.progress.is_none() {
            return (tr.get("running"), None);
        }
        let (fraction, left) = board.measure(Instant::now());
        self.progress_line = Some(fraction);
        let percent = FluentValue::from((fraction * 100.0).floor() as u64);
        let detail = format!(
            "{} · {}",
            tr.with("progress-percent", &[("percent", percent)]),
            time_left(tr, left)
        );
        (tr.get("running"), Some(detail))
    }

    /// Cancel All while the run works; New Task and Review once it is over.
    fn run_actions(&mut self, ui: &mut egui::Ui, bar: egui::Rect) {
        let Some(board) = &self.board else {
            return;
        };
        let tr = &self.tr;
        let buttons = if board.is_over() {
            match board.finished.as_ref().and_then(|f| f.session.clone()) {
                Some(session) => vec![
                    (tr.get("new-task"), ToolbarButton::Plain, Act::NewRun),
                    (
                        tr.get("review-all"),
                        ToolbarButton::Primary,
                        Act::Review(session, None),
                    ),
                ],
                None => vec![(tr.get("new-task"), ToolbarButton::Primary, Act::NewRun)],
            }
        } else if self.quit == Quit::AfterRun {
            vec![(tr.get("quit-waiting"), ToolbarButton::Text, Act::None)]
        } else {
            vec![(
                tr.get("cancel-all"),
                ToolbarButton::Outlined,
                Act::CancelRun,
            )]
        };
        let widths: Vec<f32> = buttons
            .iter()
            .map(|(text, kind, _)| match kind {
                ToolbarButton::Text => style::text_width(ui, text),
                kind => style::button_width(ui, text, *kind == ToolbarButton::Primary),
            })
            .collect();
        let spacing = ui.spacing().item_spacing.x;
        let width = widths.iter().sum::<f32>() + spacing * (widths.len() - 1) as f32;
        let right = bar.right() - 14.0;
        let mut act = Act::None;
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_x_y_ranges(
                    right - width..=right,
                    bar.y_range(),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                for (text, kind, chosen) in buttons {
                    let clicked = match kind {
                        ToolbarButton::Primary => style::primary(ui, &text, true).clicked(),
                        ToolbarButton::Outlined => style::outlined(ui, text).clicked(),
                        ToolbarButton::Plain => style::plain(ui, text).clicked(),
                        ToolbarButton::Text => {
                            ui.label(egui::RichText::new(text).color(Palette::of(ui).secondary));
                            false
                        }
                    };
                    if clicked {
                        act = chosen;
                    }
                }
            },
        );
        let ctx = ui.ctx().clone();
        self.run_act(act, &ctx);
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
                // The review holds the session; it lets go first.
                if self.review.as_ref().is_some_and(|r| r.review.id == id) {
                    self.review = None;
                    self.sessions_read = false;
                }
                match self.sessions.discard(&id) {
                    Ok(()) => self.sessions_read = false,
                    Err(e) => self.notices.push(e.to_string()),
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
            self.save_settings();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Main,
    Run,
    Review,
}

enum SidebarAct {
    Close,
    Open(SessionId),
    Discard(SessionId),
}

/// A review waiting in the sidebar: the folder it saves to, when its run was, and how many
/// items it holds; the hover and a screen reader get the whole path and what changed.
struct SessionRow {
    name: String,
    age: Option<String>,
    count: Option<usize>,
    readable: bool,
    described: String,
    detail: String,
}

impl SessionRow {
    fn of(saved: &Saved, now: SystemTime, tr: &Translator) -> Self {
        let age = saved
            .created
            .map(|created| crate::clock::when(created, now, tr));
        match &saved.content {
            Ok(loaded) => {
                let output = &loaded.session.output;
                let name = output.file_name().map_or_else(
                    || output.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                let mut detail = vec![tr.with(
                    "session-row",
                    &[
                        ("items", FluentValue::from(loaded.session.items.len())),
                        ("output", FluentValue::from(output.display().to_string())),
                    ],
                )];
                let changed = loaded.changed_count();
                if changed > 0 {
                    detail
                        .push(tr.with("session-changed", &[("count", FluentValue::from(changed))]));
                }
                let detail = detail.join(&tr.get("list-separator"));
                let described: Vec<&str> = std::iter::once(name.as_str())
                    .chain(age.as_deref())
                    .chain([detail.as_str()])
                    .collect();
                Self {
                    described: described.join(&tr.get("list-separator")),
                    count: Some(loaded.session.items.len()),
                    readable: true,
                    name,
                    age,
                    detail,
                }
            }
            Err(e) => {
                let name = tr.get("session-unreadable-short");
                let detail = tr.with(
                    "session-unreadable",
                    &[("error", FluentValue::from(e.as_str()))],
                );
                Self {
                    described: format!("{name}{}{detail}", tr.get("list-separator")),
                    count: None,
                    readable: false,
                    name,
                    age,
                    detail,
                }
            }
        }
    }
}

/// The names of the rows under How share one column, so their controls start together.
fn how_label_width(ui: &egui::Ui, tr: &Translator) -> f32 {
    ["shape", "coverage", "strength"]
        .into_iter()
        .map(|id| style::text_width(ui, &tr.get(id)))
        .fold(34.0, f32::max)
}

fn theme_preference(theme: Theme) -> egui::ThemePreference {
    match theme {
        Theme::System => egui::ThemePreference::System,
        Theme::Light => egui::ThemePreference::Light,
        Theme::Dark => egui::ThemePreference::Dark,
    }
}

/// Draws `add` inside `rect` only.
fn region<R>(ui: &mut egui::Ui, rect: egui::Rect, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            add(ui)
        },
    )
    .inner
}

/// The chosen sidebar entry is in the heavier face.
fn sidebar_text(text: &str, selected: bool, p: &Palette) -> egui::RichText {
    let text = egui::RichText::new(text).color(p.text);
    if selected {
        text.font(style::medium(13.0))
    } else {
        text
    }
}

/// A video's length over the bottom right of its thumbnail, as m:ss.
fn duration_badge(ui: &egui::Ui, picture: egui::Rect, seconds: f64) {
    let whole = seconds.round() as u64;
    let text = format!("{}:{:02}", whole / 60, whole % 60);
    let galley =
        ui.painter()
            .layout_no_wrap(text, egui::FontId::proportional(11.0), egui::Color32::WHITE);
    let size = galley.size() + egui::vec2(10.0, 2.0);
    let badge = egui::Rect::from_min_size(picture.max - size - egui::vec2(5.0, 5.0), size);
    ui.painter()
        .rect_filled(badge, 3.0, egui::Color32::from_black_alpha(178));
    ui.painter().galley(
        badge.min + egui::vec2(5.0, 1.0),
        galley,
        egui::Color32::WHITE,
    );
}

/// The button that takes one file out of the list. Every file has one so that Tab reaches it;
/// [`paint_remove_button`] shows it only where the pointer or the focus is.
fn remove_button(
    ui: &mut egui::Ui,
    index: usize,
    picture: egui::Rect,
    name: &str,
    tr: &Translator,
) -> egui::Response {
    const SIZE: f32 = 22.0;
    let rect = egui::Rect::from_min_size(
        egui::pos2(picture.right() - SIZE - 5.0, picture.top() + 5.0),
        egui::vec2(SIZE, SIZE),
    );
    let response = ui.interact(
        rect,
        ui.make_persistent_id(("remove file", index)),
        egui::Sense::click(),
    );
    let label = tr.with("found-remove", &[("name", FluentValue::from(name))]);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    response.on_hover_text(label)
}

fn paint_remove_button(ui: &egui::Ui, response: &egui::Response) {
    let alpha = if response.hovered() { 230 } else { 178 };
    ui.painter().rect_filled(
        response.rect,
        style::BUTTON_RADIUS,
        egui::Color32::from_black_alpha(alpha),
    );
    style::icon::close(
        ui.painter(),
        response.rect.shrink(5.0),
        egui::Color32::WHITE,
    );
    style::focus_ring(ui, response);
}

/// What a finished run is called: its session stays listed while its items wait for the user.
/// The list is read again only when the review closes, so an open review that is done counts.
fn result_title(
    board: &Board,
    sessions: &SavedSessions,
    review: Option<&ReviewScreen>,
) -> &'static str {
    let session = board.finished.as_ref().and_then(|f| f.session.as_ref());
    let waiting = session.is_some_and(|id| {
        sessions.list.iter().any(|saved| saved.id == *id)
            && !review.is_some_and(|r| r.review.id == *id && r.review.is_done())
    });
    if waiting {
        "result-title-review"
    } else {
        "result-title"
    }
}

/// A heading inside a tab of the settings dialog.
fn dialog_section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(12.0);
    style::group_title(ui, title);
    ui.add_space(2.0);
}

/// A row of the dialog: its name in a column of its own, the controls beside it.
fn dialog_field(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    style::field(ui, 120.0, label, add);
    ui.add_space(4.0);
}

/// The width of a check box and its gap, by which what belongs to it is set in.
const DIALOG_HINT_INDENT: f32 = 23.0;

/// What the check box above it does.
fn dialog_hint(ui: &mut egui::Ui, text: String) {
    let p = Palette::of(ui);
    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: DIALOG_HINT_INDENT as i8,
            ..Default::default()
        })
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(text).size(12.0).color(p.secondary)).wrap(),
            );
        });
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
    /// Open the session for review, at an item if given.
    Review(SessionId, Option<ItemId>),
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
        Logged::Note(note) => (None, wording::run_note(note, |item| board.name(item), tr)),
        Logged::ItemNote(item, note) => (Some(*item), wording::note(note, tr)),
        Logged::Failed(item, failure) => (Some(*item), wording::failure_with_detail(failure, tr)),
    };
    match item {
        Some(item) => format!("{time}  {}  {text}", board.name(item)),
        None => format!("{time}  {text}"),
    }
}

/// The run's log, folded away under the list (plan 6).
const ROW_HEIGHT: f32 = 52.0;
/// The engine updates a run's progress without a message that would draw the window again.
const PROGRESS_REDRAW: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, PartialEq)]
enum ToolbarButton {
    Primary,
    Outlined,
    Plain,
    Text,
}

enum RowAct {
    Cancel,
    Review,
    /// Show in the file manager, or open with `true`.
    Show(PathBuf, bool),
}

#[derive(Clone, Copy, PartialEq)]
enum Look {
    Waiting,
    Working,
    NeedsReview,
    Saved,
    Failed,
    Stopped,
}

struct RowButton {
    label: String,
    act: RowAct,
    icon: bool,
    plain: bool,
}

/// One item of the run as its row shows it.
struct RunRow {
    id: u64,
    name: String,
    state: String,
    look: Look,
    progress: Option<f32>,
    /// A few words beside the state; `full` has everything, for the pointer and screen readers.
    short: Option<String>,
    full: Vec<String>,
    described: String,
    buttons: Vec<RowButton>,
}

impl RunRow {
    fn of(
        board: &Board,
        item: ItemId,
        input: &Path,
        progress: &BTreeMap<ItemId, Progress>,
        has_session: bool,
        tr: &Translator,
    ) -> Self {
        let state = board
            .states
            .get(&item)
            .copied()
            .unwrap_or(ItemState::Queued);
        let cancelling = board.cancelling.contains(&item);
        let outcome = board
            .finished
            .as_ref()
            .and_then(|f| f.report.items.iter().find(|o| o.item == item));
        let working = !state.is_terminal() && state != ItemState::NeedsReview && !board.is_over();
        // The engine has no state for an item it is analyzing: it stays queued until analyzed.
        let analyzing = state == ItemState::Queued
            && working
            && progress.get(&item).is_some_and(|p| p.done > 0);
        let look = match state {
            ItemState::Queued if !board.is_over() && !analyzing => Look::Waiting,
            ItemState::NeedsReview => Look::NeedsReview,
            ItemState::Published(_) => Look::Saved,
            ItemState::Failed => Look::Failed,
            ItemState::Cancelled => Look::Stopped,
            _ if working => Look::Working,
            _ => Look::Stopped,
        };
        let mut problems = Vec::new();
        let mut full = Vec::new();
        let mut short = None;
        if let Some(o) = outcome {
            if let Some(failure) = &o.failure {
                short = Some(wording::failure(failure, tr));
                full.push(wording::failure_with_detail(failure, tr));
            }
            if o.state == ItemState::NeedsReview {
                let video = o.analysis.video.as_ref().map(|v| (v.frames, v.seconds));
                problems = o
                    .analysis
                    .evidence
                    .iter()
                    .filter(|e| !e.is_resolved())
                    .map(|e| summary::problem(e, video, tr))
                    .collect();
            }
            let masked = matches!(o.state, ItemState::Published(b)
                    if b != PublishBasis::ExportedWithoutAnonymization)
            .then(|| wording::masked(&o.analysis, tr));
            // The count says what these notes would.
            let notes: Vec<String> = o
                .notes
                .iter()
                .filter(|n| {
                    masked.is_none() || !matches!(n, Note::Tracked { .. } | Note::NothingDetected)
                })
                .map(|n| wording::note(n, tr))
                .collect();
            if let Some(masked) = masked {
                short = short.or_else(|| Some(masked.clone()));
                full.push(masked);
            }
            short = short
                .or_else(|| problems.first().cloned())
                .or_else(|| notes.first().cloned());
            full.extend(problems);
            full.extend(notes);
        }
        let mut buttons = Vec::new();
        if working && !cancelling {
            buttons.push(RowButton {
                label: tr.get("cancel-item"),
                act: RowAct::Cancel,
                icon: true,
                plain: true,
            });
        }
        if let Some(o) = outcome {
            match o.state {
                ItemState::Published(_) => {
                    buttons.push(RowButton {
                        label: tr.get("item-reveal"),
                        act: RowAct::Show(o.destination.clone(), false),
                        icon: false,
                        plain: true,
                    });
                    buttons.push(RowButton {
                        label: tr.get("item-open"),
                        act: RowAct::Show(o.destination.clone(), true),
                        icon: false,
                        plain: true,
                    });
                }
                ItemState::NeedsReview if has_session => buttons.push(RowButton {
                    label: tr.get("item-review"),
                    act: RowAct::Review,
                    icon: false,
                    plain: false,
                }),
                ItemState::Failed => buttons.push(RowButton {
                    label: tr.get("item-reveal-source"),
                    act: RowAct::Show(input.to_path_buf(), false),
                    icon: false,
                    plain: true,
                }),
                _ => {}
            }
        }
        let name = board.name(item);
        let state_text = tr.get(if cancelling {
            "state-cancelling"
        } else if analyzing {
            "state-analyzing"
        } else {
            state_id(state)
        });
        let described = [name.as_str(), state_text.as_str()]
            .into_iter()
            .chain(full.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(&tr.get("list-separator"));
        Self {
            id: item.0,
            name,
            state: state_text,
            described,
            look,
            progress: working
                .then(|| progress.get(&item).filter(|p| p.total > 0))
                .flatten()
                .map(|p| p.done as f32 / p.total as f32),
            short,
            full,
            buttons,
        }
    }
}

/// A row of the run: thumbnail, name, state, a few words or the progress, and its buttons.
fn run_row(
    ui: &mut egui::Ui,
    row: &RunRow,
    thumbnail: Option<&egui::TextureHandle>,
) -> Option<RowAct> {
    let p = *Palette::of(ui);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, ROW_HEIGHT), egui::Sense::hover());
    let response = ui.interact(
        rect,
        ui.make_persistent_id(("run-item", row.id)),
        egui::Sense::hover(),
    );
    if row.progress.is_some() {
        ui.painter().rect_filled(rect, 0.0, p.empty);
    }
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, p.faint_line),
    );

    let gap = 14.0;
    // The same columns while the run works and once it is over, so the state does not move
    // when the buttons of a finished row arrive.
    let (detail_width, buttons_width) = (200.0, 200.0);
    let mut x = rect.left() + 24.0;
    let mut column = |w: f32| {
        let column = egui::Rect::from_x_y_ranges(x..=x + w, rect.y_range());
        x += w + gap;
        column
    };
    let thumb = column(48.0);
    let name_width = (rect.right()
        - 24.0
        - (thumb.right() + gap)
        - 160.0
        - detail_width
        - buttons_width
        - gap * 3.0)
        .max(40.0);
    let name = column(name_width);
    let state = column(160.0);
    let detail = column(detail_width);
    let buttons =
        egui::Rect::from_x_y_ranges(detail.right() + gap..=rect.right() - 24.0, rect.y_range());

    let picture = egui::Rect::from_center_size(thumb.center(), egui::vec2(48.0, 36.0));
    match thumbnail {
        Some(texture) => {
            egui::Image::new(texture)
                .uv(style::cover(texture.size_vec2(), picture.size()))
                .corner_radius(3)
                .paint_at(ui, picture);
        }
        None => {
            ui.painter().rect_filled(picture, 3, p.separator);
        }
    }
    let font = if row.progress.is_some() {
        style::medium(13.0)
    } else {
        egui::FontId::proportional(13.0)
    };
    paint_line(ui, &row.name, font, p.text, name);

    let color = match row.look {
        Look::Waiting | Look::Stopped => p.secondary,
        Look::Working => p.text,
        Look::NeedsReview => p.needs_review,
        Look::Saved => p.saved,
        Look::Failed => p.failed,
    };
    let mark = egui::Rect::from_center_size(
        egui::pos2(state.left() + 3.5, state.center().y),
        egui::vec2(7.0, 7.0),
    );
    if row.look == Look::Waiting || row.look == Look::Stopped {
        ui.painter()
            .circle_stroke(mark.center(), 2.75, egui::Stroke::new(1.5, color));
    } else {
        ui.painter().circle_filled(mark.center(), 3.5, color);
    }
    let words = if row.look == Look::Waiting {
        p.secondary
    } else {
        p.text
    };
    paint_line(
        ui,
        &row.state,
        egui::FontId::proportional(13.0),
        words,
        egui::Rect::from_x_y_ranges(state.left() + 14.0..=state.right(), state.y_range()),
    );

    if let Some(fraction) = row.progress {
        let rail = egui::Rect::from_x_y_ranges(
            detail.left()..=detail.right() - 38.0,
            detail.center().y - 1.5..=detail.center().y + 1.5,
        );
        ui.painter().rect_filled(rail, 2, p.separator);
        let done = egui::Rect::from_x_y_ranges(
            rail.left()..=rail.left() + rail.width() * fraction.clamp(0.0, 1.0),
            rail.y_range(),
        );
        ui.painter().rect_filled(done, 2, p.text);
        ui.painter().text(
            egui::pos2(detail.right(), detail.center().y),
            egui::Align2::RIGHT_CENTER,
            format!("{}%", (fraction * 100.0).floor()),
            egui::FontId::proportional(12.0),
            p.secondary,
        );
    } else if let Some(short) = &row.short {
        paint_line(
            ui,
            short,
            egui::FontId::proportional(13.0),
            p.secondary,
            detail,
        );
    }

    let mut chosen = None;
    let spacing = 4.0;
    let widths: Vec<f32> = row
        .buttons
        .iter()
        .map(|b| {
            if b.icon {
                style::SMALL_BUTTON_HEIGHT
            } else {
                style::small_width(ui, &b.label)
            }
        })
        .collect();
    let total = widths.iter().sum::<f32>() + spacing * widths.len().saturating_sub(1) as f32;
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_x_y_ranges(
                (buttons.right() - total).max(buttons.left())..=buttons.right(),
                rect.y_range(),
            ))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = spacing;
            for (i, button) in row.buttons.iter().enumerate() {
                let response = ui.push_id(i, |ui| {
                    if button.icon {
                        style::icon_button(ui, &button.label, style::icon::close)
                    } else {
                        style::small(ui, &button.label, button.plain)
                    }
                });
                if response.inner.clicked() {
                    chosen = Some(i);
                }
            }
        },
    );
    if !row.full.is_empty() {
        response.on_hover_text(row.full.join("\n"));
    }
    chosen.map(|i| match &row.buttons[i].act {
        RowAct::Cancel => RowAct::Cancel,
        RowAct::Review => RowAct::Review,
        RowAct::Show(path, open) => RowAct::Show(path.clone(), *open),
    })
}

/// What the run came to in a sentence, the notes on it, or why it could not start.
fn run_summary(ui: &mut egui::Ui, board: &Board, tr: &Translator) -> Act {
    let notes: Vec<String> = board
        .notes
        .iter()
        .chain(board.finished.iter().flat_map(|f| f.problems.iter()))
        .map(|note| wording::run_note(note, |item| board.name(item), tr))
        .collect();
    if !board.is_over() && notes.is_empty() {
        return Act::None;
    }
    let p = *Palette::of(ui);
    let mut act = Act::None;
    let shown = egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: 24,
            right: 24,
            top: 22,
            bottom: 18,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            let headline = match (&board.finished, &board.failure) {
                (Some(finished), _) => Some(summary::sentence(
                    &summary::summarize(finished.report.items.iter().map(|o| summary::Item {
                        state: o.state,
                        evidence: &o.analysis.evidence,
                        video: o.analysis.video.as_ref().map(|v| (v.frames, v.seconds)),
                    })),
                    tr,
                )),
                (None, Some(failure)) => Some(run_failure(failure, tr)),
                (None, None) => None,
            };
            if let Some(headline) = headline {
                ui.label(
                    egui::RichText::new(headline)
                        .font(style::medium(17.0))
                        .color(p.text),
                );
            }
            if board.finished.as_ref().is_some_and(|f| f.session.is_some()) {
                ui.label(egui::RichText::new(tr.get("run-session-kept")).color(p.secondary));
            }
            for note in &notes {
                ui.label(egui::RichText::new(note).color(p.secondary));
            }
            if matches!(board.failure, Some(Failure::MissingModel(_)))
                && style::outlined(ui, tr.get("models-open")).clicked()
            {
                act = Act::Models;
            }
        });
    let rect = shown.response.rect;
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, p.separator),
    );
    act
}

fn run_failure(failure: &Failure, tr: &Translator) -> String {
    match failure {
        Failure::NoRuntime => tr.get("start-needs-runtime"),
        Failure::MissingModel(name) => {
            tr.with("model-missing", &[("name", FluentValue::from(*name))])
        }
        Failure::NoDataFolder => tr.get("run-failed-no-data-folder"),
        Failure::Other(e) => tr.with("run-failed", &[("error", FluentValue::from(e.as_str()))]),
    }
}

/// The run's log, folded under its list.
fn run_details(ui: &mut egui::Ui, board: &Board, tr: &Translator, id: egui::Id) {
    let p = *Palette::of(ui);
    let top = ui.cursor().top();
    ui.painter().hline(
        ui.max_rect().x_range(),
        top + 0.5,
        egui::Stroke::new(1.0, p.separator),
    );
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(24, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                id,
                false,
            );
            let title = tr.get("log-title");
            let header = ui.add(
                egui::Button::new(egui::RichText::new(&title).color(p.secondary)).frame(false),
            );
            if header.clicked() {
                state.toggle(ui);
                state.store(ui.ctx());
            }
            state.show_body_unindented(ui, |ui| {
                let lines: Vec<String> = board
                    .log
                    .iter()
                    .map(|(at, logged)| log_line(board, *at, logged, tr))
                    .collect();
                if style::small(ui, &tr.get("log-copy"), false).clicked() {
                    ui.ctx().copy_text(lines.join("\n"));
                }
                egui::ScrollArea::vertical()
                    .id_salt("log-lines")
                    .max_height(200.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        a11y::list(ui, &title, |ui| {
                            for line in &lines {
                                ui.label(egui::RichText::new(line).monospace().color(p.body));
                            }
                        });
                    });
            });
        });
}

/// `text` on one line in `rect`, cut short with an ellipsis if it does not fit.
fn paint_line(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    rect: egui::Rect,
) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width());
    let galley = ui.painter().layout_job(job);
    let at = egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, color);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A queued item the engine has begun reads as analyzing; one it has not reads as waiting.
    #[test]
    fn a_queued_item_with_progress_is_analyzing() {
        let tr = Translator::new(Language::En);
        let mut board = Board::new(std::time::Instant::now(), PathBuf::from("out"));
        let (started, waiting) = (ItemId(1), ItemId(2));
        board.take(run::Message::Started {
            items: vec![
                (started, PathBuf::from("clip.mp4")),
                (waiting, PathBuf::from("next.mp4")),
            ],
            progress: Default::default(),
            cancels: BTreeMap::new(),
        });
        let progress = BTreeMap::from([
            (started, Progress { done: 3, total: 10 }),
            (waiting, Progress { done: 0, total: 10 }),
        ]);
        let row = |item, input| RunRow::of(&board, item, Path::new(input), &progress, false, &tr);

        let analyzing = row(started, "clip.mp4");
        assert_eq!(analyzing.state, "Analyzing");
        assert!(matches!(analyzing.look, Look::Working));
        assert_eq!(analyzing.progress, Some(0.3));
        let queued = row(waiting, "next.mp4");
        assert_eq!(queued.state, "Waiting");
        assert!(matches!(queued.look, Look::Waiting));
    }

    /// The engine's progress comes without a message, so the run screen draws itself again
    /// while the run works.
    #[test]
    fn a_working_run_is_drawn_again_for_its_progress() {
        let mut state = AppState::new(Settings::default(), None, Vec::new());
        let mut board = Board::new(std::time::Instant::now(), PathBuf::from("out"));
        board.take(run::Message::Started {
            items: vec![(ItemId(0), PathBuf::from("clip.mp4"))],
            progress: Default::default(),
            cancels: BTreeMap::new(),
        });
        state.board = Some(board);
        let mut harness = egui_kittest::Harness::builder()
            .with_context_setup(|ctx| ctx.set_fonts(fonts::definitions(Language::En)))
            .build_ui_state(|ui, state: &mut AppState| state.ui(ui), state);
        harness.run();
        let delay = harness.output().viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        assert!(delay <= PROGRESS_REDRAW, "drawn again after {delay:?}");
    }

    /// A run of hundreds of items asks for the thumbnails of the rows on screen only.
    #[test]
    fn only_the_rows_on_screen_ask_for_thumbnails() {
        let mut state = AppState::new(Settings::default(), None, Vec::new());
        let mut board = Board::new(std::time::Instant::now(), PathBuf::from("out"));
        board.take(run::Message::Started {
            items: (0..300)
                .map(|i| (ItemId(i), PathBuf::from(format!("missing-{i}.jpg"))))
                .collect(),
            progress: Default::default(),
            cancels: BTreeMap::new(),
        });
        state.board = Some(board);
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1100.0, 760.0))
            .with_context_setup(|ctx| ctx.set_fonts(fonts::definitions(Language::En)))
            .build_ui_state(|ui, state: &mut AppState| state.ui(ui), state);
        harness.step();
        harness.step();
        let asked = harness.state().thumbnails.as_ref().unwrap().asked();
        assert!((1..20).contains(&asked), "{asked} thumbnails asked for");
    }
}
