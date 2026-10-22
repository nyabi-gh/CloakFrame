//! Tracking a video forwards and backwards and joining the two (1.12.1
//! `buildBidirectionalTracks`): the backward pass starts tracks the forward pass could not, such
//! as a face whose first frames were all weak detections.
//!
//! The join is fed both passes' assignments frame by frame in forward order. A backward track is
//! decided at its last frame, when all its overlaps with forward tracks are known, so only the
//! backward tracks still running hold state. 1.12.1 compared each backward track against the
//! forward tracks as earlier backward tracks had already extended them, which made the result
//! depend on the order; here every backward track is compared with the forward pass alone.

use std::collections::BTreeMap;

use crate::detection::FrameIndex;

use super::scene::SceneCuts;
use super::tracker::{Assignment, ByteTracker};
use super::{Observation, Track, TrackId, TrackedBox, TrackerConfig};

/// The mean IoU over the frames both tracks have a box at which a backward track joins a forward
/// one (1.12.1's 0.5).
pub const MERGE_IOU: f32 = 0.5;

/// Gathers the overlap of every running backward track with the forward tracks.
pub struct Merger {
    threshold: f32,
    /// (backward, forward) → (frames both have a box, summed IoU), for running backward tracks.
    overlaps: BTreeMap<(TrackId, TrackId), (u32, f32)>,
    joined: BTreeMap<TrackId, TrackId>,
    alone: Vec<TrackId>,
}

impl Merger {
    pub fn new(threshold: f32) -> Self {
        Self {
            threshold,
            overlaps: BTreeMap::new(),
            joined: BTreeMap::new(),
            alone: Vec::new(),
        }
    }

    /// One frame, in forward order. `backward` comes from the backward pass, whose `started`
    /// marks a track's last frame in forward order.
    pub fn frame(
        &mut self,
        detections: &[Observation],
        forward: &[Assignment],
        backward: &[Assignment],
    ) {
        for b in backward {
            for f in forward {
                let iou = detections[b.detection]
                    .bbox
                    .iou(&detections[f.detection].bbox);
                let entry = self.overlaps.entry((b.track, f.track)).or_insert((0, 0.0));
                entry.0 += 1;
                entry.1 += iou;
            }
        }
        for b in backward.iter().filter(|b| b.started) {
            let mut best: Option<(TrackId, f32)> = None;
            let range = (b.track, TrackId::MIN)..=(b.track, TrackId::MAX);
            for (&(_, f), &(shared, sum)) in self.overlaps.range(range) {
                let mean = sum / shared as f32;
                if mean >= self.threshold && best.is_none_or(|(_, m)| mean > m) {
                    best = Some((f, mean));
                }
            }
            match best {
                Some((f, _)) => {
                    self.joined.insert(b.track, f);
                }
                None => self.alone.push(b.track),
            }
            self.overlaps
                .retain(|&(backward, _), _| backward != b.track);
        }
    }

    /// Where each backward track's boxes go. Backward tracks that joined no forward track become
    /// tracks of their own, numbered from `next_id` in the backward pass's order.
    pub fn finish(mut self, next_id: TrackId) -> Joins {
        self.alone.sort_unstable();
        let mut target = self.joined;
        let after = next_id + self.alone.len() as TrackId;
        for (i, b) in self.alone.into_iter().enumerate() {
            target.insert(b, next_id + i as TrackId);
        }
        Joins { target, after }
    }
}

pub struct Joins {
    target: BTreeMap<TrackId, TrackId>,
    after: TrackId,
}

impl Joins {
    /// The number after the last of the joined tracks: the forward pass's next number and the
    /// backward tracks of their own counted on from it.
    pub fn next_id(&self) -> TrackId {
        self.after
    }

    /// The tracks' boxes in one frame: every forward box, then for each backward box in backward
    /// track order the box of the track it joined, where that track has none yet. At most one
    /// box per track, sorted by track.
    pub fn frame(
        &self,
        detections: &[Observation],
        forward: &[Assignment],
        backward: &[Assignment],
    ) -> Vec<(TrackId, Observation)> {
        let mut boxes: BTreeMap<TrackId, Observation> = forward
            .iter()
            .map(|f| (f.track, detections[f.detection]))
            .collect();
        let mut backward: Vec<&Assignment> = backward.iter().collect();
        backward.sort_unstable_by_key(|b| b.track);
        for b in backward {
            let track = self.target[&b.track];
            boxes.entry(track).or_insert(detections[b.detection]);
        }
        boxes.into_iter().collect()
    }
}

fn tracked(frame: FrameIndex, o: &Observation) -> TrackedBox {
    TrackedBox {
        frame,
        bbox: o.bbox,
        score: o.score,
        interpolated: false,
        roll: o.roll,
    }
}

/// Collects per-frame `(track, box)` pairs into tracks sorted by id.
fn collect(per_frame: impl IntoIterator<Item = (FrameIndex, TrackId, TrackedBox)>) -> Vec<Track> {
    let mut tracks: BTreeMap<TrackId, Vec<TrackedBox>> = BTreeMap::new();
    for (_, id, b) in per_frame {
        tracks.entry(id).or_default().push(b);
    }
    tracks
        .into_iter()
        .map(|(id, boxes)| Track {
            id,
            low_confidence: false,
            boxes,
        })
        .collect()
}

fn pass(
    frames: &[Vec<Observation>],
    config: &TrackerConfig,
    cut: impl Fn(FrameIndex) -> bool,
    order: impl Iterator<Item = usize>,
) -> (Vec<Vec<Assignment>>, TrackId) {
    let mut tracker = ByteTracker::new(*config, 1);
    let mut out = vec![Vec::new(); frames.len()];
    for (step, k) in order.enumerate() {
        out[k] = tracker
            .update(step as FrameIndex, cut(step as FrameIndex), &frames[k])
            .assigned;
    }
    (out, tracker.next_id())
}

/// Forward tracking of whole frames held in memory (1.12.1 `buildTracks`).
pub fn build_tracks(
    frames: &[Vec<Observation>],
    config: &TrackerConfig,
    cuts: &SceneCuts,
) -> Vec<Track> {
    let (assigned, _) = pass(frames, config, |f| cuts.is_cut(f), 0..frames.len());
    collect(assigned.iter().enumerate().flat_map(|(k, a)| {
        a.iter().map(move |a| {
            let frame = k as FrameIndex;
            (frame, a.track, tracked(frame, &frames[k][a.detection]))
        })
    }))
}

/// Forward and backward tracking of whole frames held in memory, joined (1.12.1
/// `buildBidirectionalTracks`).
pub fn build_bidirectional_tracks(
    frames: &[Vec<Observation>],
    config: &TrackerConfig,
    threshold: f32,
    cuts: &SceneCuts,
) -> Vec<Track> {
    let n = frames.len();
    let (forward, next_id) = pass(frames, config, |f| cuts.is_cut(f), 0..n);
    // Backward step s reads frame n - 1 - s, which starts a scene in reverse when frame n - s
    // starts one forwards.
    let reversed = cuts.reversed(n as FrameIndex);
    let (backward, _) = pass(frames, config, |s| reversed.is_cut(s), (0..n).rev());
    let mut merger = Merger::new(threshold);
    for k in 0..n {
        merger.frame(&frames[k], &forward[k], &backward[k]);
    }
    let joins = merger.finish(next_id);
    collect((0..n).flat_map(|k| {
        let frame = k as FrameIndex;
        joins
            .frame(&frames[k], &forward[k], &backward[k])
            .into_iter()
            .map(move |(id, o)| (frame, id, tracked(frame, &o)))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;

    fn at(x: f32) -> Observation {
        Observation {
            bbox: Rect::new(x, 0.0, 40.0, 40.0).unwrap(),
            score: 0.9,
            roll: None,
        }
    }

    fn assigned(detection: usize, track: TrackId, started: bool) -> Assignment {
        Assignment {
            detection,
            track,
            started,
        }
    }

    /// Where the forward track has a box, it keeps it; the backward track it joined fills only
    /// the frames the forward pass missed.
    #[test]
    fn a_joined_backward_track_fills_only_what_the_forward_one_missed() {
        let frames = [
            vec![at(0.0), at(5.0)],
            vec![at(2.0), at(6.0)],
            vec![at(4.0)],
        ];
        let forward = [vec![assigned(0, 1, true)], vec![], vec![]];
        let backward = [
            vec![assigned(1, 7, false)],
            vec![assigned(0, 7, false), assigned(1, 8, true)],
            vec![assigned(0, 7, true)],
        ];
        let mut merger = Merger::new(MERGE_IOU);
        for k in 0..3 {
            merger.frame(&frames[k], &forward[k], &backward[k]);
        }
        let joins = merger.finish(2);
        assert_eq!(joins.next_id(), 3);
        let boxes: Vec<Vec<(TrackId, f32)>> = (0..3)
            .map(|k| {
                joins
                    .frame(&frames[k], &forward[k], &backward[k])
                    .into_iter()
                    .map(|(id, o)| (id, o.bbox.x()))
                    .collect()
            })
            .collect();
        // Track 7 joined 1 (IoU 0.78 at frame 0); track 8 overlapped no forward box and is new.
        assert_eq!(
            boxes,
            [vec![(1, 0.0)], vec![(1, 2.0), (2, 6.0)], vec![(1, 4.0)]]
        );
    }
}
