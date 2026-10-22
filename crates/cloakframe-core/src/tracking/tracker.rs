//! ByteTrack for one detection kind (P-2: faces and plates never share a track). Frames go in
//! one at a time and each comes back as which detection continues or starts which track, so a
//! track's boxes need not be held: only the live tracks are.

use crate::detection::FrameIndex;
use crate::geometry::{Point, Rect};

use super::assign::max_weight_matching;
use super::{Observation, TrackId, TrackerConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Assignment {
    /// Index into the frame's detections.
    pub detection: usize,
    pub track: TrackId,
    /// The track's first box.
    pub started: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrameUpdate {
    pub assigned: Vec<Assignment>,
    /// Tracks that ended before this frame and will not be continued.
    pub ended: Vec<TrackId>,
}

struct Live {
    id: TrackId,
    last: Rect,
    last_frame: FrameIndex,
    last_high_frame: FrameIndex,
    velocity: Point,
}

impl Live {
    /// The last box moved by the velocity, at most its longer side.
    fn predicted(&self, frame: FrameIndex) -> Rect {
        let dt = (frame - self.last_frame) as f32;
        let (mut sx, mut sy) = (self.velocity.x * dt, self.velocity.y * dt);
        let max_shift = self.last.width().max(self.last.height());
        let length = sx.hypot(sy);
        if length > max_shift {
            sx *= max_shift / length;
            sy *= max_shift / length;
        }
        let c = self.last.center();
        self.last.centered_at(Point {
            x: c.x + sx,
            y: c.y + sy,
        })
    }
}

pub struct ByteTracker {
    config: TrackerConfig,
    live: Vec<Live>,
    next_id: TrackId,
}

impl ByteTracker {
    /// New tracks are numbered from `first_id`, so trackers for several kinds can share one
    /// numbering.
    pub fn new(config: TrackerConfig, first_id: TrackId) -> Self {
        Self {
            config,
            live: Vec::new(),
            next_id: first_id,
        }
    }

    /// The number the next new track will get.
    pub fn next_id(&self) -> TrackId {
        self.next_id
    }

    /// `frame` must increase from call to call. `cut`: the frame starts a new scene, which ends
    /// every live track.
    pub fn update(
        &mut self,
        frame: FrameIndex,
        cut: bool,
        detections: &[Observation],
    ) -> FrameUpdate {
        let mut update = FrameUpdate::default();
        if cut {
            update.ended.extend(self.live.drain(..).map(|t| t.id));
        }
        let c = self.config;
        self.live.retain(|t| {
            let scale = t.last.width().max(t.last.height());
            let moving = scale > 0.0 && t.velocity.x.hypot(t.velocity.y) > c.coast_motion * scale;
            let coast = if moving {
                c.max_frames_since_high_moving
            } else {
                c.max_frames_since_high
            };
            let keep =
                frame - t.last_frame <= c.max_frames_lost && frame - t.last_high_frame <= coast;
            if !keep {
                update.ended.push(t.id);
            }
            keep
        });

        let predicted: Vec<Rect> = self.live.iter().map(|t| t.predicted(frame)).collect();
        let mut high: Vec<usize> = Vec::new();
        let mut low: Vec<usize> = Vec::new();
        for (i, d) in detections.iter().enumerate() {
            if d.score >= c.high_score {
                high.push(i);
            } else if d.score >= c.low_score {
                low.push(i);
            }
        }
        // Confident detections first, then weak ones for the tracks still without a box.
        let mut unmatched: Vec<usize> = (0..self.live.len()).collect();
        for candidates in [&mut high, &mut low] {
            let matches = self.matches(frame, &predicted, &unmatched, candidates, detections);
            for &(t, d) in &matches {
                self.extend(t, frame, &detections[d]);
                update.assigned.push(Assignment {
                    detection: d,
                    track: self.live[t].id,
                    started: false,
                });
            }
            unmatched.retain(|u| matches.iter().all(|&(t, _)| t != *u));
            candidates.retain(|d| matches.iter().all(|&(_, m)| m != *d));
        }
        for d in high {
            let id = self.next_id;
            self.next_id += 1;
            self.live.push(Live {
                id,
                last: detections[d].bbox,
                last_frame: frame,
                last_high_frame: frame,
                velocity: Point { x: 0.0, y: 0.0 },
            });
            update.assigned.push(Assignment {
                detection: d,
                track: id,
                started: true,
            });
        }
        update.assigned.sort_unstable_by_key(|a| a.detection);
        update
    }

    /// `(live track, detection)` pairs: a detection may continue a track when it overlaps the
    /// track's predicted box enough and is not a jump in size.
    fn matches(
        &self,
        frame: FrameIndex,
        predicted: &[Rect],
        tracks: &[usize],
        candidates: &[usize],
        detections: &[Observation],
    ) -> Vec<(usize, usize)> {
        let weights: Vec<Vec<Option<f32>>> = tracks
            .iter()
            .map(|&t| {
                let live = &self.live[t];
                candidates
                    .iter()
                    .map(|&d| {
                        let bbox = detections[d].bbox;
                        let iou = predicted[t].iou(&bbox);
                        let fits = iou >= self.config.iou
                            && !size_jump_too_large(&live.last, &bbox, frame - live.last_frame);
                        fits.then_some(iou)
                    })
                    .collect()
            })
            .collect();
        max_weight_matching(&weights)
            .into_iter()
            .map(|(r, k)| (tracks[r], candidates[k]))
            .collect()
    }

    fn extend(&mut self, t: usize, frame: FrameIndex, detection: &Observation) {
        let live = &mut self.live[t];
        let dt = (frame - live.last_frame) as f32;
        if dt > 0.0 {
            let (from, to) = (live.last.center(), detection.bbox.center());
            let blend = self.config.velocity_blend;
            live.velocity = Point {
                x: live.velocity.x * (1.0 - blend) + (to.x - from.x) / dt * blend,
                y: live.velocity.y * (1.0 - blend) + (to.y - from.y) / dt * blend,
            };
        }
        live.last = detection.bbox;
        live.last_frame = frame;
        if detection.score >= self.config.high_score {
            live.last_high_frame = frame;
        }
    }

    /// Ends the live tracks.
    pub fn finish(self) -> Vec<TrackId> {
        self.live.into_iter().map(|t| t.id).collect()
    }
}

/// Whether `to` is too much larger or smaller than `from` to be the same subject `gap` frames on.
/// The allowance grows 5% a frame up to a cap (1.12.1 `sizeJumpTooLarge`).
pub(crate) fn size_jump_too_large(from: &Rect, to: &Rect, gap: FrameIndex) -> bool {
    if from.width() <= 0.0 || from.height() <= 0.0 || to.width() <= 0.0 || to.height() <= 0.0 {
        return true;
    }
    let slack = 1.0 + 0.05 * gap.saturating_sub(1) as f32;
    let dimension_limit = 2.4f32.min(1.6 * slack);
    let area_limit = 4.5f32.min(2.0 * slack * slack);
    let ratio = |a: f32, b: f32| (a / b).max(b / a);
    ratio(to.width(), from.width()) > dimension_limit
        || ratio(to.height(), from.height()) > dimension_limit
        || ratio(to.area(), from.area()) > area_limit
}
