//! The review screen (plan 6 "사진 검토"): the queue of a session's items, and the editor in which
//! a photo's masks are toggled, added, moved and previewed before the item is decided.

use std::path::PathBuf;

use cloakframe_core::coverage::{CoverageEvidence, Problem, Resolution};
use cloakframe_core::geometry::Rect;
use cloakframe_core::item::{ItemId, ItemState};
use cloakframe_engine::review::{Action, Rejection};
use cloakframe_engine::scan::MediaKind;
use cloakframe_engine::session::{SessionItem, StoredMedia};
use eframe::egui;
use fluent_bundle::FluentValue;

use crate::draft::{Doc, Draft, Pick};
use crate::i18n::Translator;
use crate::queue::{self, Group};
use crate::review::{Review, Status};
use crate::summary;
use crate::thumbnails::{Thumbnails, VideoTools};
use crate::tiles::Picture;

/// What the screen asks of the window.
pub enum ScreenAct {
    Back,
    Discard,
    /// Show in the file manager, or open with `true`.
    Show(PathBuf, bool),
}

/// An export the user has to confirm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Confirm {
    pub item: ItemId,
    pub action: Action,
}

pub struct ReviewScreen {
    pub review: Review,
    pub editor: Option<Editor>,
    thumbnails: Option<Thumbnails>,
    pub confirm: Option<Confirm>,
}

/// How the photo sits in the canvas: `scale` points per photo pixel, `center` the photo point
/// at the canvas's center.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub scale: f32,
    pub center: egui::Pos2,
}

enum Drag {
    Adding { from: egui::Pos2, to: egui::Pos2 },
    Moving { last: egui::Pos2 },
    Panning,
}

pub struct Editor {
    pub item: ItemId,
    pub picture: Option<Picture>,
    pub view: Option<View>,
    pub help: bool,
    pub previewing: bool,
    drag: Option<Drag>,
    /// The document the masking preview was last made of, and its number.
    previewed: Option<(Doc, u64)>,
    focus: bool,
}

impl Editor {
    fn new(item: &SessionItem, ctx: &egui::Context) -> Self {
        let picture = match item.media {
            StoredMedia::Photo { .. } => Picture::open(&item.input, ctx).ok(),
            StoredMedia::Video { .. } => None,
        };
        Self {
            item: item.item,
            picture,
            view: None,
            help: false,
            previewing: false,
            drag: None,
            previewed: None,
            focus: true,
        }
    }
}

impl ReviewScreen {
    pub fn new(review: Review) -> Self {
        Self {
            review,
            editor: None,
            thumbnails: None,
            confirm: None,
        }
    }

    pub fn open_item(&mut self, item: ItemId, ctx: &egui::Context) {
        if let Some(stored) = self.review.order.iter().find(|i| i.item == item) {
            self.editor = Some(Editor::new(stored, ctx));
        }
    }

    /// Waits for the queue's thumbnails, for tests.
    pub fn wait_for_thumbnails(&mut self, ctx: &egui::Context) {
        if let Some(thumbnails) = &mut self.thumbnails {
            thumbnails.wait(ctx);
        }
    }

    /// Waits for the photo in the editor and its tiles on screen, for tests.
    pub fn wait_for_picture(&mut self, ctx: &egui::Context) {
        if let Some(picture) = self.editor.as_mut().and_then(|e| e.picture.as_mut()) {
            picture.wait(ctx);
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        tr: &Translator,
        video: Option<VideoTools>,
    ) -> Option<ScreenAct> {
        let ctx = ui.ctx().clone();
        self.review.take();
        let act = if self.editor.is_some() {
            self.editor_ui(ui, tr)
        } else {
            self.queue_ui(ui, tr, video)
        };
        self.confirm_dialog(&ctx, tr);
        act
    }

    fn queue_ui(
        &mut self,
        ui: &mut egui::Ui,
        tr: &Translator,
        video: Option<VideoTools>,
    ) -> Option<ScreenAct> {
        let ctx = ui.ctx().clone();
        let mut act = None;
        let review = &self.review;
        ui.horizontal(|ui| {
            if ui.button(tr.get("session-back")).clicked() {
                act = Some(ScreenAct::Back);
            }
            if review.is_done() {
                ui.heading(tr.get("review-done"));
            } else {
                let summary =
                    summary::summarize(review.session.items.iter().map(|i| summary::Item {
                        state: ItemState::NeedsReview,
                        evidence: &i.evidence,
                        video: video_length(&i.media),
                    }));
                ui.heading(summary::sentence(&summary, tr));
            }
        });
        ui.horizontal(|ui| {
            ui.label(tr.get("output-folder"));
            ui.label(review.session.output.display().to_string());
        });
        if review.options_guessed {
            ui.colored_label(ui.visuals().warn_fg_color, tr.get("review-options-guessed"));
        }
        for problem in &review.problems {
            ui.colored_label(ui.visuals().warn_fg_color, problem);
        }
        if self.thumbnails.is_none() {
            self.thumbnails = Thumbnails::new(&ctx, video).ok();
        }
        if let Some(thumbnails) = &mut self.thumbnails {
            thumbnails.take(&ctx);
        }
        let mut open = None;
        let thumbnails = &mut self.thumbnails;
        egui::ScrollArea::vertical()
            .max_height((ui.available_height() - 40.0).max(120.0))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for item in &review.order {
                        let name = file_name(item);
                        let (cell, response) =
                            ui.allocate_exact_size(egui::vec2(148.0, 176.0), egui::Sense::click());
                        let (badge, warn) = badge(review, item, tr);
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::Role::Button,
                                true,
                                format!("{name}, {badge}"),
                            )
                        });
                        if response.clicked() {
                            open = Some(item.item);
                        }
                        if !ui.is_rect_visible(cell) {
                            continue;
                        }
                        let painter = ui.painter();
                        if response.hovered() || response.has_focus() {
                            painter.rect_filled(cell, 4.0, ui.visuals().faint_bg_color);
                        }
                        let picture = egui::Rect::from_min_size(
                            cell.min + egui::vec2(10.0, 4.0),
                            egui::vec2(128.0, 128.0),
                        );
                        let kind = match item.media {
                            StoredMedia::Photo { .. } => MediaKind::Image,
                            StoredMedia::Video { .. } => MediaKind::Video,
                        };
                        let shown = thumbnails
                            .as_mut()
                            .and_then(|t| t.get(&item.input, kind).flatten());
                        if let Some(texture) = shown {
                            let size = texture.size_vec2();
                            let fit = size * (128.0 / size.max_elem());
                            let at = egui::Rect::from_center_size(picture.center(), fit);
                            egui::Image::new(texture).paint_at(ui, at);
                        } else {
                            painter.rect_filled(picture, 4.0, ui.visuals().extreme_bg_color);
                        }
                        let color = if warn {
                            ui.visuals().warn_fg_color
                        } else {
                            ui.visuals().weak_text_color()
                        };
                        let text = ui.visuals().text_color();
                        let font = egui::FontId::proportional(12.0);
                        let name_at = egui::pos2(cell.min.x + 10.0, picture.max.y + 4.0);
                        let galley = painter.layout(name, font.clone(), text, 128.0);
                        painter.galley(name_at, galley.clone(), text);
                        painter.text(
                            name_at + egui::vec2(0.0, galley.size().y.min(16.0) + 2.0),
                            egui::Align2::LEFT_TOP,
                            badge,
                            font,
                            color,
                        );
                    }
                });
            });
        if ui.button(tr.get("session-discard")).clicked() {
            act = Some(ScreenAct::Discard);
        }
        if let Some(item) = open {
            self.open_item(item, &ctx);
        }
        act
    }

    fn editor_ui(&mut self, ui: &mut egui::Ui, tr: &Translator) -> Option<ScreenAct> {
        let ctx = ui.ctx().clone();
        let Some(editor) = &mut self.editor else {
            return None;
        };
        let item = editor.item;
        let Some(stored) = self.review.order.iter().find(|i| i.item == item).cloned() else {
            self.editor = None;
            return None;
        };
        if let Some(picture) = &mut editor.picture {
            picture.take(&ctx);
            if let (Some(draft), Some((w, h))) = (self.review.drafts.get_mut(&item), picture.size) {
                draft.bounds = Some((w as f32, h as f32));
            }
        }
        let mut asked = Asked::default();
        let open = self.review.is_open(item);
        egui::Panel::right("review-side")
            .resizable(false)
            .exact_size(320.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    side_panel(ui, tr, &mut self.review, editor, &stored, open, &mut asked);
                });
            });
        egui::CentralPanel::default().show(ui, |ui| match &stored.media {
            StoredMedia::Photo { detections, .. } => {
                if let Some(draft) = self.review.drafts.get_mut(&item) {
                    let keys = canvas(
                        ui,
                        tr,
                        editor,
                        draft,
                        detections,
                        &self.review.options,
                        open,
                    );
                    if let Some(step) = keys {
                        asked.go = Some(step);
                    }
                }
            }
            StoredMedia::Video { .. } => {
                ui.label(tr.get("editor-video"));
            }
        });
        if editor.help {
            shortcuts(&ctx, tr, &mut editor.help);
        }
        if let Some(action) = asked.decide {
            match action {
                Action::Approve => self.decide(item, action, None, &ctx),
                _ => self.confirm = Some(Confirm { item, action }),
            }
        }
        match asked.go {
            Some(Go::Queue) => self.editor = None,
            Some(Go::Step(step)) => {
                if let Some(at) = self.review.position(item) {
                    let n = self.review.order.len() as isize;
                    let next = (at as isize + step).rem_euclid(n) as usize;
                    let next = self.review.order[next].item;
                    self.open_item(next, &ctx);
                }
            }
            Some(Go::Rename(name)) => {
                if let Some(action) = self.review.last_action(item) {
                    self.decide(item, action, Some(name), &ctx);
                }
            }
            None => {}
        }
        asked.act
    }

    /// Decides the item and, once its render has started, moves on to the next item to decide
    /// while it renders; a refused decision stays on screen with the reason.
    fn decide(
        &mut self,
        item: ItemId,
        action: Action,
        rename_to: Option<String>,
        ctx: &egui::Context,
    ) {
        self.review.decide(item, action, rename_to);
        if *self.review.status(item) != Status::Rendering {
            return;
        }
        match self.review.next_open(item) {
            Some(next) => self.open_item(next, ctx),
            None => self.editor = None,
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context, tr: &Translator) {
        let Some(confirm) = self.confirm else {
            return;
        };
        let unresolved = self
            .review
            .drafts
            .get(&confirm.item)
            .map_or(0, Draft::unresolved);
        let (title, body, yes) = match confirm.action {
            Action::ExportWithoutAnonymization => (
                tr.get("confirm-plain-title"),
                tr.get("confirm-plain-body"),
                tr.get("confirm-plain"),
            ),
            _ => (
                tr.get("confirm-warnings-title"),
                tr.with(
                    "confirm-warnings-body",
                    &[("count", FluentValue::from(unresolved))],
                ),
                tr.get("confirm-warnings"),
            ),
        };
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::unique("review-confirm")).show(ctx, |ui| {
            ui.heading(title);
            ui.label(body);
            ui.horizontal(|ui| {
                if ui.button(yes).clicked() {
                    choice = Some(true);
                }
                if ui.button(tr.get("confirm-cancel")).clicked() {
                    choice = Some(false);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(false);
        }
        match choice {
            Some(true) => {
                self.confirm = None;
                self.decide(confirm.item, confirm.action, None, ctx);
            }
            Some(false) => self.confirm = None,
            None => {}
        }
    }
}

enum Go {
    Queue,
    Step(isize),
    /// Decide again under a free name the engine suggested.
    Rename(String),
}

fn file_name(item: &SessionItem) -> String {
    item.input
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn video_length(media: &StoredMedia) -> Option<(u64, f64)> {
    match media {
        StoredMedia::Video {
            frames, seconds, ..
        } => Some((*frames, *seconds)),
        StoredMedia::Photo { .. } => None,
    }
}

/// The line under a queue cell, and whether it warns.
fn badge(review: &Review, item: &SessionItem, tr: &Translator) -> (String, bool) {
    match review.status(item.item) {
        Status::Changed => (tr.get("review-badge-changed"), true),
        Status::Rendering => (tr.get("review-badge-saving"), false),
        Status::Saved(_) => (tr.get("state-published"), false),
        Status::Refused(_) | Status::Failed(_) => (tr.get("review-badge-again"), true),
        Status::Waiting => {
            let unresolved = review.drafts.get(&item.item).map_or(0, Draft::unresolved);
            match queue::group(item) {
                _ if unresolved > 0 => (
                    tr.with(
                        "review-badge-problems",
                        &[("count", FluentValue::from(unresolved))],
                    ),
                    true,
                ),
                Group::NothingDetected => (tr.get("review-badge-nothing"), false),
                _ => (tr.get("review-badge-waiting"), false),
            }
        }
    }
}

fn resolution_id(resolution: Resolution) -> &'static str {
    match resolution {
        Resolution::Unresolved => "editor-problem-open",
        Resolution::CoveredByManualMask => "editor-problem-covered",
        Resolution::Dismissed => "editor-problem-dismissed",
    }
}

fn refusal(rejection: &Rejection, tr: &Translator) -> String {
    match rejection {
        Rejection::SettingsChanged => tr.get("refused-settings"),
        Rejection::SourceChanged => tr.get("session-item-changed"),
        Rejection::Stale { .. } | Rejection::ForeignKey => tr.get("refused-stale"),
        Rejection::NameTaken { suggestion } => tr.with(
            "refused-name",
            &[("name", FluentValue::from(suggestion.display().to_string()))],
        ),
        other => tr.with(
            "refused-other",
            &[("reason", FluentValue::from(other.to_string()))],
        ),
    }
}

/// What the editor's controls asked for in a frame, done once drawing is over.
#[derive(Default)]
struct Asked {
    go: Option<Go>,
    decide: Option<Action>,
    act: Option<ScreenAct>,
}

fn side_panel(
    ui: &mut egui::Ui,
    tr: &Translator,
    review: &mut Review,
    editor: &mut Editor,
    stored: &SessionItem,
    open: bool,
    asked: &mut Asked,
) {
    let item = stored.item;
    ui.horizontal(|ui| {
        if ui.button(tr.get("editor-to-queue")).clicked() {
            asked.go = Some(Go::Queue);
        }
        if ui.button(tr.get("editor-previous")).clicked() {
            asked.go = Some(Go::Step(-1));
        }
        if ui.button(tr.get("editor-next")).clicked() {
            asked.go = Some(Go::Step(1));
        }
    });
    ui.heading(file_name(stored));
    if let Some(at) = review.position(item) {
        ui.label(tr.with(
            "editor-position",
            &[
                ("at", FluentValue::from(at + 1)),
                ("count", FluentValue::from(review.order.len())),
            ],
        ));
    }
    match review.status(item).clone() {
        Status::Waiting => {}
        Status::Changed => {
            ui.colored_label(ui.visuals().warn_fg_color, tr.get("session-item-changed"));
        }
        Status::Rendering => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(tr.get("review-badge-saving"));
            });
        }
        Status::Saved(path) => {
            ui.label(tr.get("state-published"));
            ui.horizontal(|ui| {
                if ui.button(tr.get("item-reveal")).clicked() {
                    asked.act = Some(ScreenAct::Show(path.clone(), false));
                }
                if ui.button(tr.get("item-open")).clicked() {
                    asked.act = Some(ScreenAct::Show(path.clone(), true));
                }
            });
        }
        Status::Refused(rejection) => {
            ui.colored_label(ui.visuals().error_fg_color, refusal(&rejection, tr));
            if let Rejection::NameTaken { suggestion } = &rejection
                && let Some(name) = suggestion.file_name()
            {
                let name = name.to_string_lossy().into_owned();
                let label = tr.with(
                    "refused-rename",
                    &[("name", FluentValue::from(name.as_str()))],
                );
                if ui.button(label).clicked() {
                    asked.go = Some(Go::Rename(name));
                }
            }
        }
        Status::Failed(e) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                tr.with("editor-failed", &[("error", FluentValue::from(e))]),
            );
        }
    }
    let Some(draft) = review.drafts.get_mut(&item) else {
        return;
    };
    ui.separator();
    if let StoredMedia::Photo { detections, .. } = &stored.media {
        let masked = (0..detections.len())
            .filter(|&i| !draft.is_excluded(i))
            .count();
        ui.label(tr.with(
            "editor-detections",
            &[
                ("count", FluentValue::from(detections.len())),
                ("masked", FluentValue::from(masked)),
            ],
        ));
        ui.label(tr.with(
            "editor-added",
            &[("count", FluentValue::from(draft.doc.edits.added.len()))],
        ));
    }
    ui.add_space(6.0);
    ui.strong(tr.get("editor-problems"));
    if draft.problems().is_empty() {
        ui.label(tr.get("editor-no-problems"));
    }
    let video = video_length(&stored.media);
    let mut resolve = None;
    let problems = draft.problems().to_vec();
    for (i, problem) in problems.iter().enumerate() {
        let resolution = draft.doc.resolutions[i];
        let evidence = CoverageEvidence {
            problem: problem.clone(),
            resolution,
        };
        let selected = draft.selected == Some(Pick::Problem(i));
        ui.group(|ui| {
            let text = summary::problem(&evidence, video, tr);
            if ui.selectable_label(selected, text).clicked() {
                draft.selected = Some(Pick::Problem(i));
            }
            let state = tr.get(resolution_id(resolution));
            if resolution == Resolution::Unresolved {
                ui.colored_label(ui.visuals().warn_fg_color, state);
            } else {
                ui.weak(state);
            }
            ui.add_enabled_ui(open, |ui| {
                ui.horizontal(|ui| {
                    if resolution == Resolution::Unresolved {
                        if matches!(problem, Problem::WeakCandidate { .. })
                            && matches!(stored.media, StoredMedia::Photo { .. })
                            && ui.button(tr.get("editor-cover")).clicked()
                        {
                            resolve = Some((i, Resolution::CoveredByManualMask));
                        }
                        if ui.button(tr.get("editor-dismiss")).clicked() {
                            resolve = Some((i, Resolution::Dismissed));
                        }
                    } else if ui.button(tr.get("editor-reopen")).clicked() {
                        resolve = Some((i, Resolution::Unresolved));
                    }
                });
            });
        });
    }
    match resolve {
        Some((i, Resolution::CoveredByManualMask)) => draft.cover(i),
        Some((i, resolution)) => draft.resolve(i, resolution),
        None => {}
    }
    ui.separator();
    let unresolved = draft.unresolved();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(draft.can_undo(), egui::Button::new(tr.get("editor-undo")))
            .clicked()
        {
            draft.undo();
        }
        if ui
            .add_enabled(draft.can_redo(), egui::Button::new(tr.get("editor-redo")))
            .clicked()
        {
            draft.redo();
        }
        if ui.button(tr.get("editor-shortcuts")).clicked() {
            editor.help = !editor.help;
        }
    });
    let approve = ui.add_enabled(
        open && unresolved == 0,
        egui::Button::new(tr.get("editor-approve")),
    );
    if approve.clicked() {
        asked.decide = Some(Action::Approve);
    }
    if unresolved > 0 {
        approve.on_disabled_hover_text(tr.get("editor-approve-blocked"));
        if ui
            .add_enabled(open, egui::Button::new(tr.get("editor-export-warnings")))
            .clicked()
        {
            asked.decide = Some(Action::ExportWithWarnings);
        }
    }
    ui.add_enabled_ui(open, |ui| {
        ui.menu_button(tr.get("editor-more"), |ui| {
            if ui.button(tr.get("editor-export-plain")).clicked() {
                asked.decide = Some(Action::ExportWithoutAnonymization);
            }
        });
    });
}

/// The photo with its boxes; returns a step through the queue the keys asked for.
fn canvas(
    ui: &mut egui::Ui,
    tr: &Translator,
    editor: &mut Editor,
    draft: &mut Draft,
    detections: &[cloakframe_core::detection::Detection],
    options: &crate::run::RunOptions,
    open: bool,
) -> Option<Go> {
    let Some(picture) = &mut editor.picture else {
        ui.label(tr.get("editor-loading"));
        return None;
    };
    if let Some(e) = &picture.error {
        ui.colored_label(
            ui.visuals().error_fg_color,
            tr.with(
                "editor-unreadable",
                &[("error", FluentValue::from(e.as_str()))],
            ),
        );
        return None;
    }
    let Some((width, height)) = picture.size else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(tr.get("editor-loading"));
        });
        return None;
    };
    let size = egui::vec2(width as f32, height as f32);
    let (rect, response) =
        ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Canvas, true, tr.get("editor-canvas"))
    });
    let fit = (rect.width() / size.x).min(rect.height() / size.y) * 0.95;
    let ppp = ui.ctx().pixels_per_point();
    let max_scale = 8.0 / ppp;
    let view = editor.view.get_or_insert(View {
        scale: fit,
        center: (size / 2.0).to_pos2(),
    });
    // Keys go to the photo unless another widget took them, as one does when tabbed to.
    let unfocused = ui.memory(|m| m.focused().is_none());
    if editor.focus || unfocused || response.clicked() || response.drag_started() {
        response.request_focus();
        editor.focus = false;
    }
    let focused = response.has_focus();
    if focused {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            );
        });
    }

    let origin = |v: &View| rect.center() - v.center.to_vec2() * v.scale;
    let to_photo = |v: &View, p: egui::Pos2| ((p - origin(v)) / v.scale).to_pos2();
    let to_screen = |v: &View, r: Rect| {
        egui::Rect::from_min_size(
            origin(v) + egui::vec2(r.x(), r.y()) * v.scale,
            egui::vec2(r.width(), r.height()) * v.scale,
        )
    };
    let zoom_at = |v: &mut View, factor: f32, at: egui::Pos2| {
        let fixed = to_photo(v, at);
        v.scale = (v.scale * factor).clamp(fit.min(1.0 / ppp) * 0.5, max_scale);
        v.center = fixed - (at - rect.center()) / v.scale;
    };

    if response.hovered() {
        let (zoom, scroll, pointer) = ui.input(|i| {
            (
                i.zoom_delta(),
                i.smooth_scroll_delta(),
                i.pointer.hover_pos(),
            )
        });
        if zoom != 1.0 {
            zoom_at(view, zoom, pointer.unwrap_or(rect.center()));
        } else if scroll != egui::Vec2::ZERO {
            view.center -= scroll / view.scale;
        }
    }

    // Boxes in the order a click picks them: added areas over detections over weak candidates.
    let hit = |v: &View, at: egui::Pos2, draft: &Draft| -> Option<Pick> {
        let slack = 4.0;
        let inside = |r: Rect| to_screen(v, r).expand(slack).contains(at);
        draft
            .doc
            .edits
            .added
            .iter()
            .rposition(|&r| inside(r))
            .map(Pick::Added)
            .or_else(|| {
                detections
                    .iter()
                    .rposition(|d| inside(d.bbox()))
                    .map(Pick::Detection)
            })
            .or_else(|| {
                draft
                    .problems()
                    .iter()
                    .position(|p| matches!(p, Problem::WeakCandidate { bbox, .. } if inside(*bbox)))
                    .map(Pick::Problem)
            })
    };

    if open
        && response.clicked_by(egui::PointerButton::Primary)
        && let Some(at) = response.interact_pointer_pos()
    {
        match hit(view, at, draft) {
            Some(Pick::Detection(i)) => {
                draft.toggle(i);
                draft.selected = Some(Pick::Detection(i));
            }
            picked => draft.selected = picked,
        }
    }
    if response.drag_started()
        && let Some(at) = ui.input(|i| i.pointer.press_origin())
    {
        let middle = ui.input(|i| i.pointer.middle_down() || i.pointer.secondary_down());
        editor.drag = Some(match hit(view, at, draft) {
            _ if middle || !open => Drag::Panning,
            Some(Pick::Added(i)) => {
                draft.selected = Some(Pick::Added(i));
                draft.begin_gesture();
                Drag::Moving {
                    last: to_photo(view, at),
                }
            }
            _ => Drag::Adding {
                from: to_photo(view, at),
                to: to_photo(view, at),
            },
        });
    }
    if response.dragged()
        && let Some(at) = response.interact_pointer_pos()
    {
        let now = to_photo(view, at);
        match &mut editor.drag {
            Some(Drag::Adding { to, .. }) => *to = now,
            Some(Drag::Moving { last }) => {
                let delta = now - *last;
                *last = now;
                if let Some(Pick::Added(i)) = draft.selected {
                    draft.gesture(|doc, bounds| Draft::shift(doc, i, (delta.x, delta.y), bounds));
                }
            }
            Some(Drag::Panning) => view.center -= response.drag_delta() / view.scale,
            None => {}
        }
    }
    if response.drag_stopped() {
        match editor.drag.take() {
            Some(Drag::Adding { from, to }) => {
                let area = Rect::new(
                    from.x.min(to.x),
                    from.y.min(to.y),
                    (from.x - to.x).abs(),
                    (from.y - to.y).abs(),
                );
                if let Ok(area) = area
                    && area.width() >= 2.0
                    && area.height() >= 2.0
                {
                    draft.add(area);
                }
            }
            Some(Drag::Moving { .. }) => draft.end_gesture(),
            _ => {}
        }
    }

    let mut go = None;
    if focused {
        let events = ui.input(|i| i.events.clone());
        let step = |m: egui::Modifiers| if m.shift { 10.0 } else { 1.0 };
        for event in events {
            match event {
                egui::Event::Text(t) if t == "?" => editor.help = !editor.help,
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers: m,
                    ..
                } => match key {
                    egui::Key::Escape if editor.help => editor.help = false,
                    egui::Key::Escape => draft.selected = None,
                    egui::Key::Z if m.command && m.shift => draft.redo(),
                    egui::Key::Z if m.command => draft.undo(),
                    egui::Key::Y if m.command => draft.redo(),
                    egui::Key::Tab => {
                        draft.selected = cycle(draft, detections.len(), !m.shift);
                    }
                    egui::Key::Enter if open => {
                        if let Some(Pick::Detection(i)) = draft.selected {
                            draft.toggle(i);
                        }
                    }
                    egui::Key::Delete | egui::Key::Backspace if open => {
                        if let Some(Pick::Added(i)) = draft.selected {
                            draft.remove(i);
                        }
                    }
                    egui::Key::ArrowLeft
                    | egui::Key::ArrowRight
                    | egui::Key::ArrowUp
                    | egui::Key::ArrowDown => {
                        let (dx, dy) = match key {
                            egui::Key::ArrowLeft => (-1.0, 0.0),
                            egui::Key::ArrowRight => (1.0, 0.0),
                            egui::Key::ArrowUp => (0.0, -1.0),
                            _ => (0.0, 1.0),
                        };
                        let d = (dx * step(m), dy * step(m));
                        match draft.selected {
                            Some(Pick::Added(i)) if open && m.alt => {
                                draft.change(|doc, bounds| Draft::grow(doc, i, d, bounds));
                            }
                            Some(Pick::Added(i)) if open => {
                                draft.change(|doc, bounds| Draft::shift(doc, i, d, bounds));
                            }
                            _ => view.center += egui::vec2(d.0, d.1) * 40.0 / step(m) / view.scale,
                        }
                    }
                    egui::Key::Plus | egui::Key::Equals => zoom_at(view, 2.0, rect.center()),
                    egui::Key::Minus => zoom_at(view, 0.5, rect.center()),
                    egui::Key::Num0 => {
                        *view = View {
                            scale: fit,
                            center: (size / 2.0).to_pos2(),
                        };
                    }
                    egui::Key::Num1 => zoom_at(view, 1.0 / ppp / view.scale, rect.center()),
                    egui::Key::PageDown => go = Some(Go::Step(1)),
                    egui::Key::PageUp => go = Some(Go::Step(-1)),
                    _ => {}
                },
                _ => {}
            }
        }
        editor.previewing = ui.input(|i| i.key_down(egui::Key::Space));
    } else {
        editor.previewing = false;
    }

    let masked = if editor.previewing {
        let generation = match &editor.previewed {
            Some((doc, n)) if *doc == draft.doc => *n,
            previous => {
                let n = previous.as_ref().map_or(1, |(_, n)| n + 1);
                picture.mask(n, draft.doc.edits.regions(detections), options.style());
                editor.previewed = Some((draft.doc.clone(), n));
                n
            }
        };
        Some(generation)
    } else {
        None
    };

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    let view = *view;
    picture.paint(&painter, origin(&view), view.scale, masked);
    if editor.previewing {
        painter.text(
            rect.left_top() + egui::vec2(8.0, 8.0),
            egui::Align2::LEFT_TOP,
            tr.get("editor-preview"),
            egui::FontId::proportional(14.0),
            ui.visuals().strong_text_color(),
        );
        return go;
    }
    let masked_color = egui::Color32::from_rgb(230, 70, 70);
    let added_color = egui::Color32::from_rgb(60, 150, 240);
    let weak_color = egui::Color32::from_rgb(240, 170, 40);
    let unmasked_color = egui::Color32::from_gray(160);
    let selected_stroke = |color| egui::Stroke::new(3.0, color);
    let problems = draft.problems().to_vec();
    for (i, problem) in problems.iter().enumerate() {
        if let Problem::WeakCandidate { bbox, .. } = problem {
            let open = draft.doc.resolutions[i] == Resolution::Unresolved;
            let color = if open { weak_color } else { unmasked_color };
            let width = if draft.selected == Some(Pick::Problem(i)) {
                3.0
            } else {
                1.5
            };
            dashed(
                &painter,
                to_screen(&view, *bbox),
                egui::Stroke::new(width, color),
            );
        }
    }
    for (i, d) in detections.iter().enumerate() {
        let at = to_screen(&view, d.bbox());
        let (color, fill) = if draft.is_excluded(i) {
            (unmasked_color, egui::Color32::TRANSPARENT)
        } else {
            (masked_color, masked_color.gamma_multiply(0.25))
        };
        let stroke = if draft.selected == Some(Pick::Detection(i)) {
            selected_stroke(color)
        } else {
            egui::Stroke::new(1.5, color)
        };
        painter.rect(at, 0.0, fill, stroke, egui::StrokeKind::Outside);
        if draft.is_excluded(i) {
            painter.line_segment(
                [at.left_top(), at.right_bottom()],
                egui::Stroke::new(1.0, color),
            );
        }
    }
    for (i, area) in draft.doc.edits.added.iter().enumerate() {
        let stroke = if draft.selected == Some(Pick::Added(i)) {
            selected_stroke(added_color)
        } else {
            egui::Stroke::new(1.5, added_color)
        };
        painter.rect(
            to_screen(&view, *area),
            0.0,
            added_color.gamma_multiply(0.25),
            stroke,
            egui::StrokeKind::Outside,
        );
    }
    if let Some(Drag::Adding { from, to }) = &editor.drag {
        let screen = egui::Rect::from_two_pos(
            origin(&view) + from.to_vec2() * view.scale,
            origin(&view) + to.to_vec2() * view.scale,
        );
        painter.rect_stroke(
            screen,
            0.0,
            egui::Stroke::new(1.5, added_color),
            egui::StrokeKind::Outside,
        );
    }
    go
}

/// The next (or previous) box a Tab selects: detections, then added areas, then weak candidates.
fn cycle(draft: &Draft, detections: usize, forward: bool) -> Option<Pick> {
    let mut picks: Vec<Pick> = (0..detections).map(Pick::Detection).collect();
    picks.extend((0..draft.doc.edits.added.len()).map(Pick::Added));
    picks.extend(
        draft
            .problems()
            .iter()
            .enumerate()
            .filter(|(_, p)| matches!(p, Problem::WeakCandidate { .. }))
            .map(|(i, _)| Pick::Problem(i)),
    );
    if picks.is_empty() {
        return None;
    }
    let n = picks.len();
    let at = draft
        .selected
        .and_then(|s| picks.iter().position(|&p| p == s));
    let next = match (at, forward) {
        (None, true) => 0,
        (None, false) => n - 1,
        (Some(i), true) => (i + 1) % n,
        (Some(i), false) => (i + n - 1) % n,
    };
    Some(picks[next])
}

fn dashed(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    painter.extend(egui::Shape::dashed_line(&corners, stroke, 6.0, 4.0));
}

/// The `?` overlay.
fn shortcuts(ctx: &egui::Context, tr: &Translator, open: &mut bool) {
    let undo = ctx.format_shortcut(&egui::KeyboardShortcut::new(
        egui::Modifiers::COMMAND,
        egui::Key::Z,
    ));
    let redo = ctx.format_shortcut(&egui::KeyboardShortcut::new(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::Z,
    ));
    let rows: [(String, &str); 13] = [
        (tr.get("keys-click"), "shortcut-click"),
        (tr.get("keys-drag"), "shortcut-drag"),
        ("Tab / Shift+Tab".to_owned(), "shortcut-tab"),
        ("Enter".to_owned(), "shortcut-enter"),
        (tr.get("keys-arrows"), "shortcut-arrows"),
        (tr.get("keys-alt-arrows"), "shortcut-alt-arrows"),
        ("Delete".to_owned(), "shortcut-delete"),
        (tr.get("keys-space"), "shortcut-space"),
        (undo, "shortcut-undo"),
        (redo, "shortcut-redo"),
        ("+  −  0  1".to_owned(), "shortcut-zoom"),
        ("Page Up / Page Down".to_owned(), "shortcut-pages"),
        ("?".to_owned(), "shortcut-help"),
    ];
    egui::Window::new(tr.get("shortcuts-title"))
        .id(egui::Id::unique("review-shortcuts"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            egui::Grid::new("shortcut-rows")
                .num_columns(2)
                .spacing([16.0, 6.0])
                .show(ui, |ui| {
                    for (keys, what) in rows {
                        ui.strong(keys);
                        ui.label(tr.get(what));
                        ui.end_row();
                    }
                });
            ui.label(tr.get("shortcut-scroll"));
            if ui.button(tr.get("shortcuts-close")).clicked() {
                *open = false;
            }
        });
}
