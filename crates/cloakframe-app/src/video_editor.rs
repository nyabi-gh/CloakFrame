//! The video editor (plan 6 "영상 검토"): the frame on screen with its tracks' boxes, the timeline
//! of track lanes and gaps, playback, and the tracks the reviewer draws over frames.

use cloakframe_core::coverage::{Problem, Resolution};
use cloakframe_core::detection::{DetectionKind, FrameIndex};
use cloakframe_core::geometry::Rect;
use cloakframe_core::review::{ManualTrack, VideoEdits};
use cloakframe_engine::video::record::TrackRegion;
use cloakframe_engine::video::tracking::TrackSummary;
use eframe::egui;
use fluent_bundle::FluentValue;

use crate::draft::{Draft, Pick};
use crate::editor::{Go, View};
use crate::i18n::Translator;
use crate::lanes;
use crate::video::Clip;

const MASKED: egui::Color32 = egui::Color32::from_rgb(230, 70, 70);
const ADDED: egui::Color32 = egui::Color32::from_rgb(60, 150, 240);
const WEAK: egui::Color32 = egui::Color32::from_rgb(240, 170, 40);
const UNMASKED: egui::Color32 = egui::Color32::from_gray(160);
const TIMELINE_HEIGHT: f32 = 150.0;

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
    let frames = preview.frames();
    let (w, h) = preview.size();
    let size = egui::vec2(w as f32, h as f32);
    if editor.clip.playing
        && let Some(shown) = &editor.clip.shown
    {
        editor.at = shown.frame;
    }
    let canvas_height = (ui.available_height() - TIMELINE_HEIGHT - 8.0).max(120.0);
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
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
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

    ui.add_space(4.0);
    timeline(ui, tr, editor, draft, tracks, frames);
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

/// What a click on the timeline lands on.
enum Hit {
    Gap(usize),
    Manual(usize),
    Track(usize),
}

/// The ruler with the frame under review, the gaps, the manual tracks and each kind's tracks
/// in rows. A click moves to its frame and selects what is under it.
fn timeline(
    ui: &mut egui::Ui,
    tr: &Translator,
    editor: &mut VideoEditor,
    draft: &mut Draft,
    tracks: &[TrackSummary],
    frames: u64,
) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), TIMELINE_HEIGHT),
        egui::Sense::click_and_drag(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Slider, true, tr.get("editor-timeline"))
    });
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, ui.visuals().faint_bg_color);
    let frames_f = frames.max(1) as f32;
    let x_of = |k: u64| rect.left() + (k as f32 / frames_f) * rect.width();
    let frame_at = |x: f32| {
        (((x - rect.left()) / rect.width() * frames_f)
            .floor()
            .max(0.0) as u64)
            .min(frames.saturating_sub(1))
    };
    let span = |first: u64, last: u64, top: f32, height: f32| {
        egui::Rect::from_min_max(
            egui::pos2(x_of(first), top),
            egui::pos2(x_of(last + 1).max(x_of(first) + 2.0), top + height),
        )
    };

    let ruler = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 16.0));
    let gap_row = egui::Rect::from_min_size(
        egui::pos2(rect.left(), ruler.bottom() + 2.0),
        egui::vec2(rect.width(), 10.0),
    );
    let manual_spans: Vec<(u64, u64)> = draft
        .doc
        .video
        .added_tracks
        .iter()
        .map(|t| (t.first, t.last))
        .collect();
    let manual_rows = lanes::pack(&manual_spans);
    let manual_count = manual_rows.iter().max().map_or(1, |r| r + 1);
    let track_rows: usize = editor.lanes.kinds.iter().map(|(_, n)| n).sum();
    let rows_top = gap_row.bottom() + 4.0;
    let kind_gaps = editor.lanes.kinds.len() as f32 * 4.0;
    let row_height = ((rect.bottom() - rows_top - kind_gaps) / (manual_count + track_rows) as f32)
        .clamp(2.0, 10.0);
    let manual_top = rows_top;
    let mut kind_tops = Vec::new();
    let mut top = manual_top + manual_count as f32 * row_height + 4.0;
    for (_, n) in &editor.lanes.kinds {
        kind_tops.push(top);
        top += *n as f32 * row_height + 4.0;
    }

    let hit = |p: egui::Pos2| -> Option<Hit> {
        let near = |r: egui::Rect| r.expand2(egui::vec2(3.0, 0.0)).contains(p);
        if gap_row.expand2(egui::vec2(0.0, 2.0)).contains(p) {
            return draft
                .problems()
                .iter()
                .enumerate()
                .find_map(|(i, pr)| match pr {
                    Problem::UnresolvedGap { first, last, .. }
                        if near(span(*first, *last, gap_row.top(), gap_row.height())) =>
                    {
                        Some(Hit::Gap(i))
                    }
                    _ => None,
                });
        }
        if let Some(i) = manual_spans.iter().enumerate().position(|(i, &(f, l))| {
            near(span(
                f,
                l,
                manual_top + manual_rows[i] as f32 * row_height,
                row_height,
            ))
        }) {
            return Some(Hit::Manual(i));
        }
        tracks
            .iter()
            .enumerate()
            .position(|(i, t)| {
                let (kind, row) = editor.lanes.rows[i];
                near(span(
                    t.first,
                    t.last,
                    kind_tops[kind] + row as f32 * row_height,
                    row_height,
                ))
            })
            .map(Hit::Track)
    };

    if let Some(p) = response.interact_pointer_pos()
        && (response.clicked() || response.dragged())
    {
        let k = frame_at(p.x);
        let picked = if response.clicked() { hit(p) } else { None };
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

    for (i, problem) in draft.problems().iter().enumerate() {
        if let Problem::UnresolvedGap { first, last, .. } = problem {
            let open = draft.doc.resolutions[i] == Resolution::Unresolved;
            let color = if open {
                ui.visuals().warn_fg_color
            } else {
                UNMASKED
            };
            let r = span(*first, *last, gap_row.top(), gap_row.height());
            painter.rect_filled(r, 1.0, color);
            if draft.selected == Some(Pick::Problem(i)) {
                painter.rect_stroke(
                    r.expand(1.0),
                    1.0,
                    egui::Stroke::new(1.5, ui.visuals().strong_text_color()),
                    egui::StrokeKind::Outside,
                );
            }
        }
    }
    for (i, t) in draft.doc.video.added_tracks.iter().enumerate() {
        let r = span(
            t.first,
            t.last,
            manual_top + manual_rows[i] as f32 * row_height,
            row_height - 1.0,
        );
        painter.rect_filled(r, 1.0, ADDED);
        for k in &t.keyframes {
            let x = x_of(k.frame) + 0.5;
            painter.line_segment(
                [
                    egui::pos2(x, r.top() - 1.0),
                    egui::pos2(x, r.bottom() + 1.0),
                ],
                egui::Stroke::new(1.5, ui.visuals().strong_text_color()),
            );
        }
        if draft.selected == Some(Pick::Manual(i)) {
            painter.rect_stroke(
                r.expand(1.0),
                1.0,
                egui::Stroke::new(1.5, ui.visuals().strong_text_color()),
                egui::StrokeKind::Outside,
            );
        }
    }
    for (i, t) in tracks.iter().enumerate() {
        let (kind, row) = editor.lanes.rows[i];
        let r = span(
            t.first,
            t.last,
            kind_tops[kind] + row as f32 * row_height,
            row_height - 1.0,
        );
        let color = if draft.is_track_excluded(t.id) {
            UNMASKED
        } else if t.low_confidence {
            WEAK
        } else {
            MASKED
        };
        painter.rect_filled(r, 1.0, color);
        if draft.selected == Some(Pick::Track(t.id)) {
            painter.rect_stroke(
                r.expand(1.0),
                1.0,
                egui::Stroke::new(1.5, ui.visuals().strong_text_color()),
                egui::StrokeKind::Outside,
            );
        }
    }
    let x = x_of(editor.at) + 0.5;
    painter.line_segment(
        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
        egui::Stroke::new(1.5, ui.visuals().strong_text_color()),
    );
    painter.text(
        ruler.left_center() + egui::vec2(4.0, 0.0),
        egui::Align2::LEFT_CENTER,
        tr.get("editor-timeline"),
        egui::FontId::proportional(11.0),
        ui.visuals().weak_text_color(),
    );
}
