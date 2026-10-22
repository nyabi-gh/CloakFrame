//! Post-processing of whole tracks held in memory, as 1.12.1 did it: interpolating holes,
//! smoothing, holding tracks over a few frames at their ends, and covering frames lost right
//! after a cut. [`super::stream`] gives the same result from frames in order; this is the
//! reference it is tested against and what the fixture tests call.

use std::collections::BTreeMap;

use crate::detection::FrameIndex;
use crate::geometry::Rect;

use super::scene::SceneCuts;
use super::tracker::size_jump_too_large;
use super::{PostProcessConfig, Track, TrackedBox, usable_roll};

/// Frames where a subject stayed unmasked, both ends included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UncoveredSpan {
    pub first: FrameIndex,
    pub last: FrameIndex,
}

impl UncoveredSpan {
    pub fn frame_count(&self) -> u64 {
        self.last - self.first + 1
    }
}

/// What post-processing could not cover. The kinds have different remedies and are not added
/// together.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoverageReport {
    /// Each frame once, however many tracks miss it.
    pub uncovered_spans: Vec<UncoveredSpan>,
    pub uncovered_frames: u64,
    /// Tracks with no strong detection at all, when they are not kept.
    pub dropped_tracks: u32,
}

/// Whether the move between two boxes `gap` frames apart is faster than three quarters of the
/// larger box a frame, too fast to interpolate (1.12.1 `gapMotionTooFast`).
pub(crate) fn motion_too_fast(a: &Rect, b: &Rect, gap: FrameIndex) -> bool {
    let (from, to) = (a.center(), b.center());
    let per_frame = (to.x - from.x).hypot(to.y - from.y) / (gap + 1) as f32;
    let scale = a.width().max(a.height()).max(b.width()).max(b.height());
    scale > 0.0 && per_frame > 0.75 * scale
}

/// `from` turned `t` of the way to `to` the short way round. 1.12.1's `kTwoPi` (6.2831853072F)
/// is the same f32 as `TAU`.
pub(crate) fn lerp_angle(from: f32, to: f32, t: f32) -> f32 {
    use std::f32::consts::TAU;
    let d = to - from;
    from + (d - (d / TAU).round_ties_even() * TAU) * t
}

/// The boxes between `a` and `b`, `gap` frames apart. A move too fast or a size change too large
/// means the path is unknown, so every frame gets the union of both ends.
pub(crate) fn fill(a: &TrackedBox, b: &TrackedBox, gap: FrameIndex) -> Vec<TrackedBox> {
    let unreliable =
        motion_too_fast(&a.bbox, &b.bbox, gap) || size_jump_too_large(&a.bbox, &b.bbox, gap);
    let conservative = a.bbox.union(&b.bbox);
    (1..=gap)
        .map(|step| {
            let t = step as f32 / (gap + 1) as f32;
            let roll = match (unreliable, a.roll, b.roll) {
                (false, Some(from), Some(to)) => usable_roll(lerp_angle(from, to, t)),
                _ => None,
            };
            TrackedBox {
                frame: a.frame + step,
                bbox: if unreliable {
                    conservative
                } else {
                    a.bbox.lerp(&b.bbox, t)
                },
                score: a.score.min(b.score),
                interpolated: true,
                roll,
            }
        })
        .collect()
}

/// Fills the holes inside a track up to `max_gap` frames and returns the longer ones. A hole
/// across a cut is neither filled nor reported: the subject left.
pub fn interpolate_gaps(track: &mut Track, max_gap: u64, cuts: &SceneCuts) -> Vec<UncoveredSpan> {
    if track.boxes.len() < 2 || max_gap < 1 {
        return Vec::new();
    }
    let mut uncovered = Vec::new();
    let mut filled = Vec::with_capacity(track.boxes.len());
    for pair in track.boxes.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        filled.push(*a);
        let gap = b.frame - a.frame - 1;
        if gap < 1 || cuts.spans_cut(a.frame, b.frame) {
            continue;
        }
        if gap > max_gap {
            uncovered.push(UncoveredSpan {
                first: a.frame + 1,
                last: b.frame - 1,
            });
            continue;
        }
        filled.extend(fill(a, b, gap));
    }
    filled.push(*track.boxes.last().expect("two or more boxes"));
    track.boxes = filled;
    uncovered
}

/// The smoothed box `i` of `boxes`: the mean of its neighbours within `radius` frames in an
/// unbroken run, grown to hold the box itself. A mean that would grow the box by more than a
/// quarter leaves it as it was. No box shrinks (P-6: 1.12.1 let interpolated boxes shrink to the
/// mean, uncovering part of the subject).
pub(crate) fn smoothed(boxes: &[TrackedBox], i: usize, radius: u64) -> TrackedBox {
    let r = radius as usize;
    let begin = i.saturating_sub(r);
    let end = (boxes.len() - 1).min(i + r);
    let (mut x, mut y, mut w, mut h) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut count = 0;
    let (mut roll_sin, mut roll_cos, mut roll_count) = (0.0f32, 0.0f32, 0);
    for (j, other) in boxes.iter().enumerate().take(end + 1).skip(begin) {
        if other.frame as i64 - boxes[i].frame as i64 != j as i64 - i as i64 {
            continue;
        }
        x += other.bbox.x();
        y += other.bbox.y();
        w += other.bbox.width();
        h += other.bbox.height();
        count += 1;
        if let Some(roll) = other.roll {
            roll_sin += roll.sin();
            roll_cos += roll.cos();
            roll_count += 1;
        }
    }
    let mut out = boxes[i];
    if count < 2 {
        return out;
    }
    if roll_count >= 2
        && let Some(roll) = usable_roll(roll_sin.atan2(roll_cos))
    {
        out.roll = Some(roll);
    }
    let inverse = 1.0 / count as f32;
    let mean = Rect::from_parts(x * inverse, y * inverse, w * inverse, h * inverse);
    let original = boxes[i].bbox;
    let covered = mean.union(&original);
    out.bbox = if original.area() > 0.0 && covered.area() > original.area() * 1.25 {
        original
    } else {
        covered
    };
    out
}

pub fn smooth_track(track: &mut Track, radius: u64) {
    if radius < 1 || track.boxes.len() < 3 {
        return;
    }
    track.boxes = (0..track.boxes.len())
        .map(|i| smoothed(&track.boxes, i, radius))
        .collect();
}

/// The first box held over the `frames` frames before it and the last over the `frames` after,
/// within the video and the same scene.
pub fn extend_track_ends(track: &mut Track, frames: u64, frame_count: u64, cuts: &SceneCuts) {
    let (Some(&first), Some(&last)) = (track.boxes.first(), track.boxes.last()) else {
        return;
    };
    if frames < 1 || frame_count < 1 {
        return;
    }
    let held = |b: TrackedBox, frame| TrackedBox {
        frame,
        interpolated: true,
        ..b
    };
    let prefix = (first.frame.saturating_sub(frames)..first.frame)
        .filter(|&f| !cuts.spans_cut(f, first.frame))
        .map(|f| held(first, f));
    let suffix = (last.frame + 1..=(frame_count - 1).min(last.frame + frames))
        .take_while(|&f| !cuts.spans_cut(last.frame, f))
        .map(|f| held(last, f));
    let mut boxes: Vec<TrackedBox> = prefix.collect();
    boxes.append(&mut track.boxes);
    boxes.extend(suffix);
    track.boxes = boxes;
}

/// Few strong detections: fewer than `min_strong`, and not a short burst that is mostly strong.
pub(crate) fn is_low_confidence(strong: u32, real: u32, config: &PostProcessConfig) -> bool {
    if strong >= config.min_strong {
        return false;
    }
    let clean_short_burst = strong >= config.short_track_min_strong
        && strong as f32 >= config.short_track_strong_ratio * real as f32;
    !clean_short_burst
}

/// Merges spans that touch or overlap, ascending.
pub(crate) fn merge_spans(mut spans: Vec<UncoveredSpan>) -> Vec<UncoveredSpan> {
    spans.sort_by_key(|s| (s.first, s.last));
    let mut merged: Vec<UncoveredSpan> = Vec::with_capacity(spans.len());
    for span in spans {
        match merged.last_mut() {
            Some(m) if span.first <= m.last + 1 => m.last = m.last.max(span.last),
            _ => merged.push(span),
        }
    }
    merged
}

/// Marks low-confidence tracks, drops tracks with no strong detection unless they are kept, and
/// interpolates, smooths and extends the rest. A cut ends every track, so the frames right after
/// one belong to none: where the tracks either side of a cut overlap, a hole no longer than an
/// interpolated one is covered by both boxes, each track taking the frames on its side so that
/// excluding one in review cannot uncover the other's subject; a longer hole is reported.
pub fn post_process_tracks(
    tracks: &mut Vec<Track>,
    config: &PostProcessConfig,
    frame_count: u64,
    cuts: &SceneCuts,
) -> CoverageReport {
    let mut report = CoverageReport::default();
    for track in tracks.iter_mut() {
        let real = track.boxes.iter().filter(|b| !b.interpolated);
        let strong = real
            .clone()
            .filter(|b| b.score >= config.strong_score)
            .count();
        track.low_confidence = is_low_confidence(strong as u32, real.count() as u32, config);
    }
    if !config.retain_without_strong {
        let before = tracks.len();
        tracks.retain(|t| {
            t.boxes
                .iter()
                .any(|b| !b.interpolated && b.score >= config.strong_score)
        });
        report.dropped_tracks = (before - tracks.len()) as u32;
    }
    let mut spans = Vec::new();
    for track in tracks.iter_mut() {
        spans.extend(interpolate_gaps(track, config.max_interpolation_gap, cuts));
        smooth_track(track, config.smoothing_radius);
        extend_track_ends(track, config.extension_frames, frame_count, cuts);
    }

    let mut by_start: Vec<usize> = (0..tracks.len())
        .filter(|&i| !tracks[i].boxes.is_empty())
        .collect();
    by_start.sort_by_key(|&i| tracks[i].boxes[0].frame);
    let mut bridged_starts: BTreeMap<usize, (FrameIndex, TrackedBox)> = BTreeMap::new();
    for &before in &by_start {
        let end = *tracks[before].boxes.last().expect("non-empty");
        let Some(cut) = cuts.first_cut_after(end.frame) else {
            continue;
        };
        for &after in &by_start {
            let start = tracks[after].boxes[0];
            if start.frame <= end.frame + 1 {
                continue;
            }
            let gap = start.frame - end.frame - 1;
            if gap > config.max_cut_boundary_gap {
                break;
            }
            if cut > start.frame || end.bbox.intersection_area(&start.bbox) <= 0.0 {
                continue;
            }
            if gap <= config.max_interpolation_gap {
                let bridge = TrackedBox {
                    frame: 0,
                    bbox: end.bbox.union(&start.bbox),
                    score: end.score.min(start.score),
                    interpolated: true,
                    roll: None,
                };
                tracks[before]
                    .boxes
                    .extend((end.frame + 1..cut).map(|frame| TrackedBox { frame, ..bridge }));
                bridged_starts
                    .entry(after)
                    .and_modify(|(first, b)| {
                        *first = (*first).min(cut);
                        b.bbox = b.bbox.union(&bridge.bbox);
                        b.score = b.score.min(bridge.score);
                    })
                    .or_insert((cut, bridge));
            } else {
                spans.push(UncoveredSpan {
                    first: end.frame + 1,
                    last: start.frame - 1,
                });
            }
            break;
        }
    }
    for (i, (first, b)) in bridged_starts {
        let start = tracks[i].boxes[0].frame;
        let prefix = (first..start).map(|frame| TrackedBox { frame, ..b });
        tracks[i].boxes.splice(0..0, prefix);
    }

    report.uncovered_spans = merge_spans(spans);
    report.uncovered_frames = report
        .uncovered_spans
        .iter()
        .map(UncoveredSpan::frame_count)
        .sum();
    report
}
