//! The video editor (plan 6 "영상 검토"): the frame on screen with its tracks' boxes, the timeline
//! of track lanes and gaps, playback, and the tracks the reviewer draws over frames.

use cloakframe_core::coverage::{Problem, Resolution};
use cloakframe_core::detection::{DetectionKind, FrameIndex};
use cloakframe_core::geometry::Rect;
use cloakframe_core::review::{ManualTrack, VideoEdits};
use cloakframe_engine::video::preview::Preview;
use cloakframe_engine::video::record::TrackRegion;
use cloakframe_engine::video::tracking::TrackSummary;
use eframe::egui;
use fluent_bundle::FluentValue;

use crate::draft::{Draft, Pick};
use crate::editor::{Go, View, kind_name};
use crate::i18n::Translator;
use crate::lanes;
use crate::style::{self, Palette};
use crate::video::Clip;

const MASKED: egui::Color32 = egui::Color32::from_rgb(230, 70, 70);
const ADDED: egui::Color32 = egui::Color32::from_rgb(60, 150, 240);
const UNMASKED: egui::Color32 = egui::Color32::from_gray(160);
const TIMELINE_HEIGHT: f32 = 150.0;
const PLAY_ROW_HEIGHT: f32 = 48.0;

enum Drag {
    Adding { from: egui::Pos2, to: egui::Pos2 },
    Moving { track: usize, last: egui::Pos2 },
    Panning,
}

/// Which row of the timeline each track is drawn in, by kind.
struct Lanes {
    kinds: Vec<(DetectionKind, usize)>,
    /// For each track, its kind's place in `kinds` and its row there.
    rows: Vec<(usize, usize)>,
}

impl Lanes {
    fn of(tracks: &[TrackSummary]) -> Self {
        let mut kinds: Vec<(DetectionKind, usize)> = Vec::new();
        let mut rows = vec![(0, 0); tracks.len()];
        for kind in [DetectionKind::Face, DetectionKind::Plate] {
            let of_kind: Vec<usize> = (0..tracks.len())
                .filter(|&i| tracks[i].kind == kind)
                .collect();
            if of_kind.is_empty() {
                continue;
            }
            let spans: Vec<(u64, u64)> = of_kind
                .iter()
                .map(|&i| (tracks[i].first, tracks[i].last))
                .collect();
            let packed = lanes::pack(&spans);
            for (&i, &row) in of_kind.iter().zip(&packed) {
                rows[i] = (kinds.len(), row);
            }
            kinds.push((kind, packed.iter().max().map_or(0, |r| r + 1)));
        }
        Self { kinds, rows }
    }
}

pub struct VideoEditor {
    pub clip: Clip,
    /// The frame under review.
    pub at: FrameIndex,
    view: Option<View>,
    drag: Option<Drag>,
    /// Playback shows the masked result; off, the frames as decoded.
    pub masked_playback: bool,
    /// M is held: the frame on screen is masked.
    pub holding_mask: bool,
    regions: Option<(FrameIndex, Vec<TrackRegion>)>,
    lanes: Lanes,
    focus: bool,
}

impl VideoEditor {
    pub fn new(clip: Clip, tracks: &[TrackSummary]) -> Self {
        Self {
            clip,
            at: 0,
            view: None,
            drag: None,
            masked_playback: true,
            holding_mask: false,
            regions: None,
            lanes: Lanes::of(tracks),
            focus: true,
        }
    }

    /// How the frame sits in the canvas, once it is drawn.
    pub fn view(&self) -> Option<View> {
        self.view
    }

    /// Moves to frame `k`, stopping playback.
    pub fn seek(&mut self, k: FrameIndex, frames: u64) {
        self.clip.stop();
        self.at = k.min(frames.saturating_sub(1));
    }

    pub fn toggle_playback(&mut self, draft: &Draft) {
        if self.clip.playing {
            self.clip.stop();
            return;
        }
        if self.clip.preview.is_none() {
            return;
        }
        let from = if self.at + 1 >= draft.frames() {
            0
        } else {
            self.at
        };
        self.at = from;
        let edits = self.masked_playback.then(|| draft.doc.video.clone());
        self.clip.play(from, edits);
    }

    /// The tracked regions of the frame under review, read once for each frame.
    pub fn tracked(&mut self) -> &[TrackRegion] {
        let at = self.at;
        if self.regions.as_ref().is_none_or(|(k, _)| *k != at) {
            let regions = self
                .clip
                .preview
                .as_ref()
                .and_then(|p| p.regions(at, &VideoEdits::default()).ok())
                .map(|r| r.tracked)
                .unwrap_or_default();
            self.regions = Some((at, regions));
        }
        self.regions.as_ref().map_or(&[], |(_, r)| r)
    }

    /// The boxes of the tracks either side of a gap, at the frames just before and after it,
    /// for a manual track over it to start from.
    pub fn around_gap(&self, problem: &Problem) -> (Option<Rect>, Option<Rect>) {
        let (
            Some(preview),
            Problem::UnresolvedGap {
                tracks,
                first,
                last,
                ..
            },
        ) = (&self.clip.preview, problem)
        else {
            return (None, None);
        };
        let box_at = |k: Option<u64>| {
            let k = k.filter(|&k| k < preview.frames())?;
            let regions = preview.regions(k, &VideoEdits::default()).ok()?;
            regions
                .tracked
                .iter()
                .find(|r| tracks.contains(&r.track))
                .map(|r| r.bbox)
        };
        (box_at(first.checked_sub(1)), box_at(last.checked_add(1)))
    }
}

/// A manual track's box at `k`, cut to the frame. Unlike `ManualTrack::box_at` it needs no
/// checked track: keyframes out of order give no box.
pub fn manual_box(track: &ManualTrack, k: FrameIndex, (w, h): (f32, f32)) -> Option<Rect> {
    let r = track.rect_at(k)?;
    crate::draft::clamp(r, Some((w, h)))
}

pub fn time(seconds: f64) -> String {
    let tenths = (seconds.max(0.0) * 10.0).round() as u64;
    format!("{}:{:02}.{}", tenths / 600, tenths / 10 % 60, tenths % 10)
}

/// The frame with its boxes over the timeline; returns a step through the queue the keys asked
/// for.
pub(crate) fn ui(
    ui: &mut egui::Ui,
    tr: &Translator,
    editor: &mut VideoEditor,
    draft: &mut Draft,
    tracks: &[TrackSummary],
    open: bool,
    help: &mut bool,
) -> Option<Go> {
    let ctx = ui.ctx().clone();
    editor.clip.take(&ctx);
    if let Some(e) = &editor.clip.error {
        ui.colored_label(
            ui.visuals().error_fg_color,
            tr.with(
                "editor-video-unreadable",
                &[("error", FluentValue::from(e.as_str()))],
            ),
        );
        return None;
    }
    let Some(preview) = editor.clip.preview.clone() else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(tr.get("editor-video-loading"));
        });
        return None;
    };
    let (w, h) = preview.size();
    let size = egui::vec2(w as f32, h as f32);
    if editor.clip.playing
        && let Some(shown) = &editor.clip.shown
    {
        editor.at = shown.frame;
    }
    let canvas_height = (ui.available_height() - TIMELINE_HEIGHT - PLAY_ROW_HEIGHT).max(120.0);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), canvas_height),
        egui::Sense::click_and_drag(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Canvas, true, tr.get("editor-video-canvas"))
    });
    let fit = (rect.width() / size.x).min(rect.height() / size.y) * 0.95;
    let ppp = ui.ctx().pixels_per_point();
    let max_scale = 8.0 / ppp;
    let mut view = editor.view.take().unwrap_or(View {
        scale: fit,
        center: (size / 2.0).to_pos2(),
    });
    let view = &mut view;
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
    let to_frame = |v: &View, p: egui::Pos2| ((p - origin(v)) / v.scale).to_pos2();
    let to_screen = |v: &View, r: Rect| {
        egui::Rect::from_min_size(
            origin(v) + egui::vec2(r.x(), r.y()) * v.scale,
            egui::vec2(r.width(), r.height()) * v.scale,
        )
    };
    let zoom_at = |v: &mut View, factor: f32, at: egui::Pos2| {
        let fixed = to_frame(v, at);
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

    let at = editor.at;
    let bounds = (size.x, size.y);
    let tracked: Vec<TrackRegion> = editor.tracked().to_vec();
    let manual: Vec<(usize, Rect)> = draft
        .doc
        .video
        .added_tracks
        .iter()
        .enumerate()
        .filter_map(|(i, t)| manual_box(t, at, bounds).map(|b| (i, b)))
        .collect();
    // Manual boxes over tracked ones, as a click picks them.
    let hit = |v: &View, p: egui::Pos2| -> Option<Pick> {
        let inside = |r: Rect| to_screen(v, r).expand(4.0).contains(p);
        manual
            .iter()
            .rev()
            .find(|(_, b)| inside(*b))
            .map(|(i, _)| Pick::Manual(*i))
            .or_else(|| {
                tracked
                    .iter()
                    .rev()
                    .find(|r| inside(r.bbox))
                    .map(|r| Pick::Track(r.track))
            })
    };
    let view_now = *view;
    if response.clicked_by(egui::PointerButton::Primary)
        && let Some(p) = response.interact_pointer_pos()
    {
        editor.clip.stop();
        match hit(&view_now, p) {
            Some(Pick::Track(id)) if open => {
                draft.toggle_track(id);
                draft.selected = Some(Pick::Track(id));
            }
            picked => draft.selected = picked,
        }
    }
    if response.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
    {
        editor.clip.stop();
        let middle = ui.input(|i| i.pointer.middle_down() || i.pointer.secondary_down());
        editor.drag = Some(match hit(&view_now, p) {
            _ if middle || !open => Drag::Panning,
            Some(Pick::Manual(i)) => {
                draft.selected = Some(Pick::Manual(i));
                draft.begin_gesture();
                Drag::Moving {
                    track: i,
                    last: to_frame(&view_now, p),
                }
            }
            _ => Drag::Adding {
                from: to_frame(&view_now, p),
                to: to_frame(&view_now, p),
            },
        });
    }
    if response.dragged()
        && let Some(p) = response.interact_pointer_pos()
    {
        let now = to_frame(view, p);
        match &mut editor.drag {
            Some(Drag::Adding { to, .. }) => *to = now,
            Some(Drag::Moving { track, last }) => {
                let delta = now - *last;
                *last = now;
                let track = *track;
                draft.gesture(|doc, b| {
                    Draft::shift_track(doc, track, at, (delta.x, delta.y), b);
                });
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
                    draft.add_track(at, area);
                }
            }
            Some(Drag::Moving { .. }) => draft.end_gesture(),
            _ => {}
        }
    }

    let mut go = None;
    if focused {
        let canvas = Canvas { fit, size, open };
        go = keys(ui, editor, draft, view, &canvas, (&tracked, &manual), help);
        editor.holding_mask = ui.input(|i| i.key_down(egui::Key::M));
    } else {
        editor.holding_mask = false;
    }
    // What to show is asked for once this frame's input has moved or changed it.
    let masking = editor.holding_mask && !editor.clip.playing;
    if !editor.clip.playing {
        if masking {
            editor.clip.show_masked(editor.at, &draft.doc.video);
        } else {
            editor.clip.show(editor.at);
        }
    }
    let at = editor.at;
    let tracked: Vec<TrackRegion> = editor.tracked().to_vec();
    let manual: Vec<(usize, Rect)> = draft
        .doc
        .video
        .added_tracks
        .iter()
        .enumerate()
        .filter_map(|(i, t)| manual_box(t, at, bounds).map(|b| (i, b)))
        .collect();

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Palette::of(ui).canvas);
    let view = *view;
    editor.view = Some(view);
    let frame_rect = to_screen(
        &view,
        Rect::new(0.0, 0.0, size.x, size.y).expect("a video has a size"),
    );
    if let Some(shown) = &editor.clip.shown {
        egui::Image::new(&shown.texture).paint_at(ui, frame_rect);
    }
    let showing_masked = editor
        .clip
        .shown
        .as_ref()
        .is_some_and(|s| s.masked && (editor.clip.playing || masking));
    if editor.clip.is_waiting() || editor.clip.shown.is_none() {
        painter.text(
            rect.right_top() + egui::vec2(-8.0, 8.0),
            egui::Align2::RIGHT_TOP,
            tr.get("editor-video-decoding"),
            egui::FontId::proportional(12.0),
            ui.visuals().weak_text_color(),
        );
    }
    if let Some(e) = &editor.clip.frame_error {
        painter.text(
            rect.left_bottom() + egui::vec2(8.0, -8.0),
            egui::Align2::LEFT_BOTTOM,
            tr.with(
                "editor-video-frame-failed",
                &[("error", FluentValue::from(e.as_str()))],
            ),
            egui::FontId::proportional(13.0),
            ui.visuals().error_fg_color,
        );
    }
    if showing_masked || editor.clip.playing {
        if showing_masked {
            painter.text(
                rect.left_top() + egui::vec2(8.0, 8.0),
                egui::Align2::LEFT_TOP,
                tr.get("editor-preview"),
                egui::FontId::proportional(14.0),
                ui.visuals().strong_text_color(),
            );
        }
    } else {
        for r in &tracked {
            let excluded = draft.is_track_excluded(r.track);
            let (color, fill) = if excluded {
                (UNMASKED, egui::Color32::TRANSPARENT)
            } else {
                (MASKED, MASKED.gamma_multiply(0.25))
            };
            let width = if draft.selected == Some(Pick::Track(r.track)) {
                3.0
            } else {
                1.5
            };
            let at = to_screen(&view, r.bbox);
            painter.rect(
                at,
                0.0,
                fill,
                egui::Stroke::new(width, color),
                egui::StrokeKind::Outside,
            );
            if excluded {
                painter.line_segment(
                    [at.left_top(), at.right_bottom()],
                    egui::Stroke::new(1.0, color),
                );
            }
        }
        for (i, b) in &manual {
            let keyframe = draft.doc.video.added_tracks[*i]
                .keyframes
                .iter()
                .any(|k| k.frame == at);
            let width = if draft.selected == Some(Pick::Manual(*i)) {
                3.0
            } else {
                1.5
            };
            let screen = to_screen(&view, *b);
            if keyframe {
                painter.rect(
                    screen,
                    0.0,
                    ADDED.gamma_multiply(0.25),
                    egui::Stroke::new(width, ADDED),
                    egui::StrokeKind::Outside,
                );
            } else {
                painter.rect_filled(screen, 0.0, ADDED.gamma_multiply(0.15));
                dashed(&painter, screen, egui::Stroke::new(width, ADDED));
            }
        }
        if let Some(Drag::Adding { from, to }) = &editor.drag {
            let screen = egui::Rect::from_two_pos(
                origin(&view) + from.to_vec2() * view.scale,
                origin(&view) + to.to_vec2() * view.scale,
            );
            painter.rect_stroke(
                screen,
                0.0,
                egui::Stroke::new(1.5, ADDED),
                egui::StrokeKind::Outside,
            );
        }
    }

    play_row(ui, tr, editor, draft, &preview, help);
    timeline(ui, tr, editor, draft, tracks, &preview);
    go
}

/// What the keys need of the canvas: the scale that fits the frame, the frame's size, and
/// whether the item can still be edited.
struct Canvas {
    fit: f32,
    size: egui::Vec2,
    open: bool,
}

/// The canvas's keys; returns a step through the queue.
fn keys(
    ui: &egui::Ui,
    editor: &mut VideoEditor,
    draft: &mut Draft,
    view: &mut View,
    &Canvas { fit, size, open }: &Canvas,
    (tracked, manual): (&[TrackRegion], &[(usize, Rect)]),
    help: &mut bool,
) -> Option<Go> {
    let frames = draft.frames();
    let second = draft.second();
    let ppp = ui.ctx().pixels_per_point();
    let mut go = None;
    let events = ui.input(|i| i.events.clone());
    for event in events {
        match event {
            egui::Event::Text(t) if t == "?" => *help = !*help,
            egui::Event::Text(t) if t == "[" || t == "]" => {
                if let Some((problem, first)) = gap_from(draft, editor.at, t == "]") {
                    draft.selected = Some(Pick::Problem(problem));
                    editor.seek(first, frames);
                }
            }
            egui::Event::Key {
                key,
                pressed: true,
                modifiers: m,
                ..
            } => {
                let step = if m.shift { 10.0 } else { 1.0 };
                match key {
                    egui::Key::Escape if *help => *help = false,
                    egui::Key::Escape => draft.selected = None,
                    egui::Key::Space => editor.toggle_playback(draft),
                    egui::Key::Z if m.command && m.shift => draft.redo(),
                    egui::Key::Z if m.command => draft.undo(),
                    egui::Key::Y if m.command => draft.redo(),
                    egui::Key::Tab => draft.selected = cycle(draft, tracked, manual, !m.shift),
                    egui::Key::Enter if open => {
                        if let Some(Pick::Track(id)) = draft.selected {
                            draft.toggle_track(id);
                        }
                    }
                    egui::Key::Delete | egui::Key::Backspace if open => {
                        if let Some(Pick::Manual(i)) = draft.selected {
                            draft.remove_track(i);
                        }
                    }
                    egui::Key::ArrowLeft
                    | egui::Key::ArrowRight
                    | egui::Key::ArrowUp
                    | egui::Key::ArrowDown
                        if m.alt =>
                    {
                        let (dx, dy) = match key {
                            egui::Key::ArrowLeft => (-step, 0.0),
                            egui::Key::ArrowRight => (step, 0.0),
                            egui::Key::ArrowUp => (0.0, -step),
                            _ => (0.0, step),
                        };
                        if let (Some(Pick::Manual(i)), true) = (draft.selected, open) {
                            let at = editor.at;
                            editor.clip.stop();
                            if m.command {
                                draft.change(|doc, b| Draft::grow_track(doc, i, at, (dx, dy), b));
                            } else {
                                draft.change(|doc, b| Draft::shift_track(doc, i, at, (dx, dy), b));
                            }
                        }
                    }
                    egui::Key::ArrowLeft => {
                        let by = if m.shift { second } else { 1 };
                        editor.seek(editor.at.saturating_sub(by), frames);
                    }
                    egui::Key::ArrowRight => {
                        let by = if m.shift { second } else { 1 };
                        editor.seek(editor.at.saturating_add(by), frames);
                    }
                    egui::Key::Home => editor.seek(0, frames),
                    egui::Key::End => editor.seek(frames.saturating_sub(1), frames),
                    egui::Key::Plus | egui::Key::Equals => zoom(view, 2.0, fit, ppp),
                    egui::Key::Minus => zoom(view, 0.5, fit, ppp),
                    egui::Key::Num0 => {
                        *view = View {
                            scale: fit,
                            center: (size / 2.0).to_pos2(),
                        };
                    }
                    egui::Key::Num1 => {
                        let factor = 1.0 / ppp / view.scale;
                        zoom(view, factor, fit, ppp);
                    }
                    egui::Key::PageDown => go = Some(Go::Step(1)),
                    egui::Key::PageUp => go = Some(Go::Step(-1)),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    go
}

/// Zooms about the canvas's center, the point `view.center` shows there.
fn zoom(view: &mut View, factor: f32, fit: f32, ppp: f32) {
    view.scale = (view.scale * factor).clamp(fit.min(1.0 / ppp) * 0.5, 8.0 / ppp);
}

/// The gap before or after frame `at` (wrapping), and its first frame.
fn gap_from(draft: &Draft, at: FrameIndex, forward: bool) -> Option<(usize, FrameIndex)> {
    let mut gaps: Vec<(FrameIndex, usize)> = draft
        .problems()
        .iter()
        .enumerate()
        .filter_map(|(i, p)| match p {
            Problem::UnresolvedGap { first, .. } => Some((*first, i)),
            _ => None,
        })
        .collect();
    gaps.sort_unstable();
    let pick = if forward {
        gaps.iter().find(|(f, _)| *f > at).or(gaps.first())
    } else {
        gaps.iter().rev().find(|(f, _)| *f < at).or(gaps.last())
    };
    pick.map(|&(first, i)| (i, first))
}

/// The next (or previous) box in the frame a Tab selects: tracked boxes, then manual ones.
fn cycle(
    draft: &Draft,
    tracked: &[TrackRegion],
    manual: &[(usize, Rect)],
    forward: bool,
) -> Option<Pick> {
    let mut picks: Vec<Pick> = tracked.iter().map(|r| Pick::Track(r.track)).collect();
    picks.extend(manual.iter().map(|(i, _)| Pick::Manual(*i)));
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

/// Play or stop, where playback is, whether it shows the masked result, and the way to every
/// shortcut.
fn play_row(
    ui: &mut egui::Ui,
    tr: &Translator,
    editor: &mut VideoEditor,
    draft: &Draft,
    preview: &Preview,
    help: &mut bool,
) {
    let p = *Palette::of(ui);
    let frames = preview.frames();
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(20, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let (label, draw): (String, fn(&egui::Painter, egui::Rect, egui::Color32)) =
                    if editor.clip.playing {
                        (tr.get("editor-video-stop"), style::icon::stop)
                    } else {
                        (tr.get("editor-video-play"), style::icon::play)
                    };
                if style::toolbar_icon(ui, &label, true, draw).clicked() {
                    editor.toggle_playback(draft);
                }
                let mut job = egui::text::LayoutJob::default();
                let font = egui::FontId::proportional(13.0);
                job.append(
                    &time(preview.seconds(editor.at)),
                    0.0,
                    egui::TextFormat::simple(font.clone(), p.text),
                );
                let total = tr.with(
                    "editor-video-total",
                    &[
                        (
                            "total",
                            FluentValue::from(time(preview.seconds(frames.saturating_sub(1)))),
                        ),
                        ("frame", FluentValue::from(editor.at)),
                    ],
                );
                job.append(
                    &format!(" {total}"),
                    0.0,
                    egui::TextFormat::simple(font, p.secondary),
                );
                ui.label(job);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let shortcuts = tr.get("editor-shortcuts");
                    let link = ui.add(
                        egui::Button::new(
                            egui::RichText::new(shortcuts).size(12.0).color(p.secondary),
                        )
                        .frame(false),
                    );
                    if link.clicked() {
                        *help = !*help;
                    }
                    ui.label(egui::RichText::new("M").color(p.secondary));
                    ui.checkbox(&mut editor.masked_playback, tr.get("editor-video-masked"));
                });
            });
        });
}

/// What a click on the timeline lands on.
enum Hit {
    Gap(usize),
    Manual(usize),
    Track(usize),
}

/// The ruler, each kind's tracks in rows, the drawn tracks with their keyframes, the gaps across
/// them and the frame under review. A click moves to its frame and selects what is under it.
fn timeline(
    ui: &mut egui::Ui,
    tr: &Translator,
    editor: &mut VideoEditor,
    draft: &mut Draft,
    tracks: &[TrackSummary],
    preview: &Preview,
) {
    let p = *Palette::of(ui);
    let frames = preview.frames();
    let whole = ui.available_rect_before_wrap();
    ui.painter().hline(
        whole.x_range(),
        whole.top(),
        egui::Stroke::new(1.0, p.faint_line),
    );
    let outer = egui::Rect::from_min_size(
        whole.min + egui::vec2(20.0, 6.0),
        egui::vec2(whole.width() - 40.0, TIMELINE_HEIGHT - 20.0),
    );
    ui.allocate_rect(
        egui::Rect::from_min_size(whole.min, egui::vec2(whole.width(), TIMELINE_HEIGHT)),
        egui::Sense::hover(),
    );
    let names = [
        kind_name(DetectionKind::Face, tr),
        kind_name(DetectionKind::Plate, tr),
        tr.get("editor-kind-drawn"),
    ];
    let gutter = names
        .into_iter()
        .map(|name| {
            ui.painter()
                .layout_no_wrap(
                    name,
                    egui::FontId::proportional(LANE_NAME_SIZE),
                    p.secondary,
                )
                .size()
                .x
                + 8.0
        })
        .fold(GUTTER, f32::max);
    let rect = egui::Rect::from_x_y_ranges(outer.left() + gutter..=outer.right(), outer.y_range());
    let response = ui.interact(
        rect,
        ui.make_persistent_id("timeline"),
        egui::Sense::click_and_drag(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Slider, true, tr.get("editor-timeline"))
    });
    let painter = ui.painter_at(outer.expand(2.0));
    let frames_f = frames.max(1) as f32;
    let x_of = |k: f32| rect.left() + (k / frames_f) * rect.width();
    let frame_at = |x: f32| {
        (((x - rect.left()) / rect.width() * frames_f)
            .floor()
            .max(0.0) as u64)
            .min(frames.saturating_sub(1))
    };
    let span = |first: u64, last: u64, top: f32, height: f32| {
        egui::Rect::from_min_max(
            egui::pos2(x_of(first as f32), top),
            egui::pos2(
                x_of((last + 1) as f32).max(x_of(first as f32) + 2.0),
                top + height,
            ),
        )
    };

    let ruler = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 18.0));
    let manual_spans: Vec<(u64, u64)> = draft
        .doc
        .video
        .added_tracks
        .iter()
        .map(|t| (t.first, t.last))
        .collect();
    let manual_rows = lanes::pack(&manual_spans);
    let manual_count = manual_rows.iter().max().map_or(0, |r| r + 1);
    let mut groups: Vec<(String, usize)> = editor
        .lanes
        .kinds
        .iter()
        .map(|&(kind, n)| (kind_name(kind, tr), n))
        .collect();
    if manual_count > 0 {
        groups.push((tr.get("editor-kind-drawn"), manual_count));
    }
    let rows: usize = groups.iter().map(|(_, n)| n).sum();
    let rows_top = ruler.bottom();
    let group_gap = 8.0;
    let lane_gap = 3.0;
    let room = rect.bottom() - rows_top - group_gap * groups.len().saturating_sub(1) as f32;
    let row_height = (room / rows.max(1) as f32 - lane_gap).clamp(4.0, 18.0);
    let row_step = row_height + lane_gap;
    let mut group_tops = Vec::new();
    let mut top = rows_top + lane_gap;
    for (_, n) in &groups {
        group_tops.push(top);
        top += *n as f32 * row_step + group_gap;
    }
    let lanes_bottom = (top - group_gap).max(rows_top + row_step);
    let track_tops: Vec<f32> = editor
        .lanes
        .rows
        .iter()
        .map(|&(kind, row)| group_tops[kind] + row as f32 * row_step)
        .collect();
    let track_top = |i: usize| track_tops[i];
    let manual_top = |i: usize| group_tops[groups.len() - 1] + manual_rows[i] as f32 * row_step;
    let band = |first: u64, last: u64| span(first, last, rows_top, lanes_bottom - rows_top);

    let hit = |p: egui::Pos2| -> Option<Hit> {
        let near = |r: egui::Rect| r.expand2(egui::vec2(3.0, 1.0)).contains(p);
        if let Some(i) = (0..manual_spans.len()).find(|&i| {
            let (f, l) = manual_spans[i];
            near(span(f, l, manual_top(i), row_height))
        }) {
            return Some(Hit::Manual(i));
        }
        if let Some(i) = (0..tracks.len()).find(|&i| {
            near(span(
                tracks[i].first,
                tracks[i].last,
                track_top(i),
                row_height,
            ))
        }) {
            return Some(Hit::Track(i));
        }
        draft
            .problems()
            .iter()
            .enumerate()
            .find_map(|(i, pr)| match pr {
                Problem::UnresolvedGap { first, last, .. } if near(band(*first, *last)) => {
                    Some(Hit::Gap(i))
                }
                _ => None,
            })
    };

    if let Some(at) = response.interact_pointer_pos()
        && (response.clicked() || response.dragged())
    {
        let k = frame_at(at.x);
        let picked = if response.clicked() { hit(at) } else { None };
        match picked {
            Some(Hit::Gap(i)) => {
                draft.selected = Some(Pick::Problem(i));
                editor.seek(k, frames);
            }
            Some(Hit::Manual(i)) => {
                draft.selected = Some(Pick::Manual(i));
                let t = &draft.doc.video.added_tracks[i];
                editor.seek(k.clamp(t.first, t.last), frames);
            }
            Some(Hit::Track(i)) => {
                let t = &tracks[i];
                draft.selected = Some(Pick::Track(t.id));
                editor.seek(k.clamp(t.first, t.last), frames);
            }
            None => editor.seek(k, frames),
        }
    }
    if response.has_focus() {
        let (left, right) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
            )
        });
        if left {
            editor.seek(editor.at.saturating_sub(1), frames);
        }
        if right {
            editor.seek(editor.at + 1, frames);
        }
    }

    let total = preview.seconds(frames.saturating_sub(1));
    let step = ruler_step(total, rect.width());
    let mut s = 0.0;
    while s <= total {
        let x = x_of((s / total.max(f64::EPSILON) * frames.saturating_sub(1) as f64) as f32);
        painter.text(
            egui::pos2(x + 4.0, ruler.center().y),
            egui::Align2::LEFT_CENTER,
            ruler_label(s),
            egui::FontId::proportional(10.0),
            p.secondary,
        );
        s += step;
    }
    for ((label, _), &top) in groups.iter().zip(&group_tops) {
        painter.text(
            egui::pos2(outer.left(), top + row_height / 2.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(LANE_NAME_SIZE),
            p.secondary,
        );
    }
    for (i, problem) in draft.problems().iter().enumerate() {
        if let Problem::UnresolvedGap { first, last, .. } = problem {
            let open = draft.doc.resolutions[i] == Resolution::Unresolved;
            let color = if open { p.needs_review } else { p.border };
            let r = band(*first, *last);
            hatch(&painter, r, color);
            let width = if draft.selected == Some(Pick::Problem(i)) {
                2.0
            } else {
                1.0
            };
            let edge = egui::Stroke::new(width, color);
            painter.vline(r.left(), r.y_range(), edge);
            painter.vline(r.right(), r.y_range(), edge);
        }
    }
    for (i, t) in tracks.iter().enumerate() {
        let r = span(t.first, t.last, track_top(i), row_height);
        let selected = draft.selected == Some(Pick::Track(t.id));
        if draft.is_track_excluded(t.id) {
            painter.rect(
                r,
                3,
                p.window,
                egui::Stroke::new(1.0, p.border),
                egui::StrokeKind::Inside,
            );
        } else if t.low_confidence {
            painter.rect_filled(r, 3, p.separator);
            dashed(
                &painter,
                r.shrink(0.75),
                egui::Stroke::new(1.5, p.secondary),
            );
        } else {
            painter.rect_filled(r, 3, if selected { p.text } else { p.secondary });
        }
        if selected && (draft.is_track_excluded(t.id) || t.low_confidence) {
            painter.rect_stroke(
                r,
                3,
                egui::Stroke::new(1.5, p.text),
                egui::StrokeKind::Outside,
            );
        }
    }
    for (i, t) in draft.doc.video.added_tracks.iter().enumerate() {
        let r = span(t.first, t.last, manual_top(i), row_height);
        let selected = draft.selected == Some(Pick::Manual(i));
        painter.rect_filled(r, 3, if selected { p.text } else { p.secondary });
        for k in &t.keyframes {
            let c = egui::pos2(x_of(k.frame as f32 + 0.5), r.center().y);
            let d = (row_height / 2.0).min(5.0);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    c + egui::vec2(0.0, -d),
                    c + egui::vec2(d, 0.0),
                    c + egui::vec2(0.0, d),
                    c + egui::vec2(-d, 0.0),
                ],
                p.window,
                egui::Stroke::new(1.5, p.text),
            ));
        }
    }
    let x = x_of(editor.at as f32 + 0.5);
    painter.vline(
        x,
        ruler.top()..=lanes_bottom,
        egui::Stroke::new(2.0, p.text),
    );
    if response.has_focus() {
        painter.rect_stroke(
            outer.expand(1.0),
            style::BUTTON_RADIUS,
            egui::Stroke::new(1.5, p.text),
            egui::StrokeKind::Outside,
        );
    }
}

/// Room at the timeline's left for each row group's name, unless a name needs more.
const GUTTER: f32 = 70.0;
const LANE_NAME_SIZE: f32 = 11.0;

/// Seconds between the ruler's marks: the shortest of a few round steps that leaves room for
/// the labels.
fn ruler_step(total: f64, width: f32) -> f64 {
    const STEPS: [f64; 10] = [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0];
    let per_second = width as f64 / total.max(1.0);
    STEPS
        .into_iter()
        .find(|s| s * per_second >= 60.0)
        .unwrap_or(STEPS[STEPS.len() - 1])
}

fn ruler_label(seconds: f64) -> String {
    let whole = seconds.round() as u64;
    if whole < 60 {
        format!("{whole}s")
    } else {
        format!("{}:{:02}", whole / 60, whole % 60)
    }
}

/// Diagonal stripes over a light wash of `color`, as a gap no track covers is marked.
fn hatch(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    painter.rect_filled(rect, 0.0, color.gamma_multiply(0.12));
    let stroke = egui::Stroke::new(1.0, color.gamma_multiply(0.45));
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        painter.line_segment(
            [
                egui::pos2(x, rect.bottom()),
                egui::pos2(x + rect.height(), rect.top()),
            ],
            stroke,
        );
        x += 6.0;
    }
}
