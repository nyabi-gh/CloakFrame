//! Video tracking (plan 4.4): scene cuts, ByteTrack for one detection kind at a time, the
//! bidirectional merge and the post-processing that turn per-frame detections into the regions
//! to mask and the gaps a reviewer must resolve. Every step takes frames in order and keeps only
//! what is still open, so the engine can stream them from its record files (M5).

mod assign;
pub mod merge;
pub mod post;
pub mod scene;
pub mod stream;
pub mod tracker;

use crate::detection::{FrameIndex, MAX_ROLL};
use crate::geometry::Rect;
use crate::timeline::Rational;

pub use crate::coverage::TrackId;

/// One detection as the tracker sees it, in source display pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Observation {
    pub bbox: Rect,
    pub score: f32,
    /// Face roll in radians when the landmarks gave a plausible one.
    pub roll: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackedBox {
    pub frame: FrameIndex,
    pub bbox: Rect,
    pub score: f32,
    /// Filled in by tracking rather than seen by the detector.
    pub interpolated: bool,
    pub roll: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub id: TrackId,
    /// Few confident detections. Still masked whole; review shows it (1.12.1).
    pub low_confidence: bool,
    /// In frame order, at most one per frame.
    pub boxes: Vec<TrackedBox>,
}

impl Track {
    pub fn first_frame(&self) -> Option<FrameIndex> {
        self.boxes.first().map(|b| b.frame)
    }

    pub fn last_frame(&self) -> Option<FrameIndex> {
        self.boxes.last().map(|b| b.frame)
    }

    pub fn box_at(&self, frame: FrameIndex) -> Option<&TrackedBox> {
        self.boxes
            .binary_search_by_key(&frame, |b| b.frame)
            .ok()
            .map(|i| &self.boxes[i])
    }
}

/// Every track's box in `frame`.
pub fn regions_for_frame(tracks: &[Track], frame: FrameIndex) -> Vec<TrackedBox> {
    tracks
        .iter()
        .filter_map(|t| t.box_at(frame).copied())
        .collect()
}

/// A roll the masks can follow (1.12.1 `isValidFacePose`).
pub(crate) fn usable_roll(roll: f32) -> Option<f32> {
    (roll.is_finite() && roll.abs() <= MAX_ROLL).then_some(roll)
}

/// ByteTrack's settings, in frames. The defaults are [`TrackingWindows`] at 30 frames per second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackerConfig {
    /// Detections at or above this start and keep tracks; weaker ones down to `low_score` only
    /// continue them.
    pub high_score: f32,
    pub low_score: f32,
    /// The least overlap between a track's predicted box and a detection that may continue it.
    pub iou: f32,
    pub max_frames_lost: u64,
    pub max_frames_since_high: u64,
    /// A moving track may coast on weak detections longer than a still one.
    pub max_frames_since_high_moving: u64,
    /// Speed per frame, as a share of the box's longer side, above which a track is moving.
    pub coast_motion: f32,
    /// How much of each new step's motion the velocity takes in.
    pub velocity_blend: f32,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        Self {
            high_score: 0.5,
            low_score: 0.1,
            iou: 0.3,
            max_frames_lost: 30,
            max_frames_since_high: 30,
            max_frames_since_high_moving: 45,
            coast_motion: 0.05,
            velocity_blend: 0.3,
        }
    }
}

/// Post-processing settings, in frames. The defaults are [`TrackingWindows`] at 30 frames per
/// second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PostProcessConfig {
    /// Longer holes inside a track, or across a cut to a track in the same place, are reported.
    pub max_interpolation_gap: u64,
    pub smoothing_radius: u64,
    /// Frames a track is held over before its first and after its last box.
    pub extension_frames: u64,
    /// How far after a cut a track in the same place is still taken for the one before it.
    pub max_cut_boundary_gap: u64,
    pub strong_score: f32,
    /// Strong detections that make a track confident whatever its length.
    pub min_strong: u32,
    /// A shorter track is confident with at least this many strong detections making at least
    /// `short_track_strong_ratio` of its detections.
    pub short_track_min_strong: u32,
    pub short_track_strong_ratio: f32,
    /// Keep tracks with no strong detection, marked low confidence, instead of dropping them.
    pub retain_without_strong: bool,
}

impl Default for PostProcessConfig {
    fn default() -> Self {
        Self {
            max_interpolation_gap: 30,
            smoothing_radius: 2,
            extension_frames: 3,
            max_cut_boundary_gap: 30,
            strong_score: 0.5,
            min_strong: 3,
            short_track_min_strong: 1,
            short_track_strong_ratio: 0.5,
            retain_without_strong: false,
        }
    }
}

/// The time limits of tracking, in seconds, so a clip is treated alike at any frame rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackingWindows {
    /// Two seconds, where 1.12.1 gave one: a subject missed for longer started a new track, and
    /// the frames between were published bare without a warning (docs/M5_RENDER.md).
    pub max_lost: f64,
    pub max_since_high: f64,
    pub max_since_high_moving: f64,
    /// As long as `max_lost`: a hole the tracker bridges by finding the same subject again is
    /// filled, not reported. 1.12.1 filled only 2/3 s and reported the rest, which every clip
    /// with people measured in M5 hit (docs/M5_PLAN.md).
    pub max_interpolation: f64,
    pub extension: f64,
    pub max_cut_boundary: f64,
}

impl Default for TrackingWindows {
    fn default() -> Self {
        Self {
            max_lost: 2.0,
            max_since_high: 2.0,
            max_since_high_moving: 2.0,
            max_interpolation: 2.0,
            extension: 0.1,
            max_cut_boundary: 2.0,
        }
    }
}

impl TrackingWindows {
    /// The frame counts at `rate`, rounded half away from zero as 1.12.1's `lround`.
    pub fn apply(&self, rate: Rational, tracker: &mut TrackerConfig, post: &mut PostProcessConfig) {
        let rate = rate.as_f64();
        let frames = |seconds: f64, minimum: u64| {
            let scaled = if seconds.is_finite() {
                (seconds * rate).clamp(0.0, 1_000_000.0)
            } else {
                0.0
            };
            (scaled.round() as u64).max(minimum)
        };
        tracker.max_frames_lost = frames(self.max_lost, 1);
        tracker.max_frames_since_high = frames(self.max_since_high, 1);
        tracker.max_frames_since_high_moving = frames(self.max_since_high_moving, 1);
        post.max_interpolation_gap = frames(self.max_interpolation, 1);
        post.extension_frames = frames(self.extension, 0);
        post.max_cut_boundary_gap = frames(self.max_cut_boundary, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(num: u32, den: u32) -> (TrackerConfig, PostProcessConfig) {
        let (mut tracker, mut post) = (TrackerConfig::default(), PostProcessConfig::default());
        TrackingWindows::default().apply(Rational::new(num, den).unwrap(), &mut tracker, &mut post);
        (tracker, post)
    }

    /// The window counts of every tracking window at a rate.
    fn counts(num: u32, den: u32) -> [u64; 6] {
        let (tracker, post) = at(num, den);
        [
            tracker.max_frames_lost,
            tracker.max_frames_since_high,
            tracker.max_frames_since_high_moving,
            post.max_interpolation_gap,
            post.extension_frames,
            post.max_cut_boundary_gap,
        ]
    }

    /// tests/fixtures/legacy/tracking_tests/testTrackingWindowsFollowTheFrameRate.json, with the
    /// windows of M5 (docs/M5_PLAN.md, docs/M5_RENDER.md): two seconds to find a subject again
    /// and to fill the hole, where 1.12.1 had one second and 2/3 (1.5 s once moving).
    #[test]
    fn windows_follow_the_frame_rate() {
        assert_eq!(counts(30, 1), [60, 60, 60, 60, 3, 60]);
        assert_eq!(counts(60, 1), [120, 120, 120, 120, 6, 120]);
        assert_eq!(counts(24, 1), [48, 48, 48, 48, 2, 48]);
        // Rates a frame apart in name give the same frame counts.
        assert_eq!(at(24000, 1001), at(24, 1));
        assert_eq!(at(30000, 1001), at(30, 1));
        // Other fields stay as configured.
        let (tracker, post) = at(30, 1);
        assert_eq!(tracker.high_score, TrackerConfig::default().high_score);
        assert_eq!(
            post.smoothing_radius,
            PostProcessConfig::default().smoothing_radius
        );
    }

    #[test]
    fn rolls_beyond_sixty_degrees_are_not_followed() {
        assert_eq!(usable_roll(0.4), Some(0.4));
        assert_eq!(usable_roll(-1.0), Some(-1.0));
        assert_eq!(usable_roll(1.1), None);
        assert_eq!(usable_roll(f32::NAN), None);
    }
}
