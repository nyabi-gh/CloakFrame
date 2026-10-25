use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::coverage::TrackId;
use crate::detection::{Detection, FrameIndex};
use crate::geometry::Rect;
use crate::mask::Region;

/// SHA-256 of an input, model set, settings or timeline, computed by the engine. Stored as
/// lowercase hex.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Digest(pub [u8; 32]);

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({self})")
    }
}

impl std::str::FromStr for Digest {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const INVALID: &str = "expected 64 hex digits";
        if s.len() != 64 || !s.is_ascii() {
            return Err(INVALID);
        }
        let mut bytes = [0; 32];
        for (byte, pair) in bytes.iter_mut().zip(s.as_bytes().chunks(2)) {
            let pair = std::str::from_utf8(pair).map_err(|_| INVALID)?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| INVALID)?;
        }
        Ok(Self(bytes))
    }
}

impl Serialize for Digest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// What a review was made against. Changing any part invalidates earlier review responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReviewKey {
    pub input: Digest,
    pub models: Digest,
    pub settings: Digest,
    pub timeline: Digest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReviewRevision {
    key: ReviewKey,
    number: u64,
}

impl ReviewRevision {
    pub fn first(key: ReviewKey) -> Self {
        Self { key, number: 0 }
    }

    #[must_use]
    pub fn next(self) -> Self {
        Self {
            key: self.key,
            number: self.number + 1,
        }
    }

    pub fn key(&self) -> ReviewKey {
        self.key
    }

    pub fn number(&self) -> u64 {
        self.number
    }

    /// A response computed for an older revision, or for different inputs, must not be applied.
    /// The CLI's decision files carry only the revision number (docs/M0_M5_REVIEW.md, item 9).
    pub fn accepts(&self, response: &ReviewRevision) -> bool {
        self == response
    }
}

/// A reviewer's changes to a photo's automatic result, in source display pixels: detections not
/// to mask, and areas detection missed.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PhotoEdits {
    /// Indices into the item's detections.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded: Vec<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditError {
    NoSuchDetection(usize),
    /// An added area with no width or height, which would mask nothing.
    EmptyArea(usize),
    NoSuchTrack(TrackId),
    ManualTrack {
        track: usize,
        error: ManualTrackError,
    },
    /// Edits of a photo given for a video, or the other way round.
    WrongMedia,
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchDetection(i) => write!(f, "the item has no detection #{i}"),
            Self::EmptyArea(i) => write!(f, "added area #{i} is empty"),
            Self::NoSuchTrack(id) => write!(f, "the item has no track {id}"),
            Self::ManualTrack { track, error } => write!(f, "added track #{track}: {error}"),
            Self::WrongMedia => f.write_str(
                "excluded and added are for photos, excluded_tracks and added_tracks for videos",
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualTrackError {
    NoKeyframes,
    /// Keyframes must be in increasing frame order, each frame once, within the track's frames.
    KeyframeOrder,
    /// The track's last frame is past the video's last.
    PastTheEnd,
    /// The track's box lies outside the frame (or has no size) at this frame.
    Outside(FrameIndex),
}

impl fmt::Display for ManualTrackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoKeyframes => f.write_str("it has no keyframes"),
            Self::KeyframeOrder => f.write_str(
                "keyframes must be in increasing frame order, between its first and last frame",
            ),
            Self::PastTheEnd => f.write_str("it ends after the video's last frame"),
            Self::Outside(frame) => write!(f, "its box lies outside the frame at frame {frame}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyframe {
    pub frame: FrameIndex,
    pub bbox: Rect,
}

/// A track the reviewer drew over source frames `first..=last`: boxes at keyframes, linearly
/// interpolated between them and held before the first and after the last (1.12.1
/// `VideoReviewManualTrack`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManualTrack {
    pub first: FrameIndex,
    pub last: FrameIndex,
    pub keyframes: Vec<Keyframe>,
}

/// A manual track's box at one frame, clipped to the frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ManualBox {
    pub bbox: Rect,
    /// Not one of the keyframes.
    pub interpolated: bool,
}

impl ManualTrack {
    fn ordered(&self) -> Result<(), ManualTrackError> {
        if self.keyframes.is_empty() {
            return Err(ManualTrackError::NoKeyframes);
        }
        let in_order = self.first <= self.last
            && self.keyframes.windows(2).all(|w| w[0].frame < w[1].frame)
            && self
                .keyframes
                .first()
                .is_some_and(|k| k.frame >= self.first)
            && self.keyframes.last().is_some_and(|k| k.frame <= self.last);
        if !in_order {
            return Err(ManualTrackError::KeyframeOrder);
        }
        match self.keyframes.iter().find(|k| k.bbox.area() <= 0.0) {
            Some(k) => Err(ManualTrackError::Outside(k.frame)),
            None => Ok(()),
        }
    }

    /// The box at `frame` before clipping, `None` outside the track (1.12.1
    /// `manualTrackRectAtFrame`). Interpolated in f64 like 1.12.1's QRectF.
    pub fn rect_at(&self, frame: FrameIndex) -> Option<Rect> {
        if self.ordered().is_err() || frame < self.first || frame > self.last {
            return None;
        }
        Some(self.unchecked_rect_at(frame))
    }

    fn unchecked_rect_at(&self, frame: FrameIndex) -> Rect {
        let next = self.keyframes.partition_point(|k| k.frame < frame);
        let (Some(after), Some(before)) = (
            self.keyframes.get(next),
            next.checked_sub(1).and_then(|i| self.keyframes.get(i)),
        ) else {
            let held = self
                .keyframes
                .get(next)
                .or(self.keyframes.last())
                .expect("checked: keyframes are not empty");
            return held.bbox;
        };
        if after.frame == frame {
            return after.bbox;
        }
        let amount = (frame - before.frame) as f64 / (after.frame - before.frame) as f64;
        let lerp = |a: f32, b: f32| (f64::from(a) + (f64::from(b) - f64::from(a)) * amount) as f32;
        let (a, b) = (before.bbox, after.bbox);
        Rect::new(
            lerp(a.x(), b.x()),
            lerp(a.y(), b.y()),
            lerp(a.width(), b.width()),
            lerp(a.height(), b.height()),
        )
        .expect("between two finite boxes of positive size")
    }

    /// The box at `frame` of a [checked](Self::check) track, clipped to a `width`×`height`
    /// frame; `None` outside the track or where nothing of the box is left in the frame.
    pub fn box_at(&self, frame: FrameIndex, (width, height): (u32, u32)) -> Option<ManualBox> {
        if self.keyframes.is_empty() || frame < self.first || frame > self.last {
            return None;
        }
        let rect = self.unchecked_rect_at(frame);
        let left = rect.x().max(0.0);
        let top = rect.y().max(0.0);
        let right = rect.right().min(width as f32);
        let bottom = rect.bottom().min(height as f32);
        let clipped = Rect::new(left, top, right - left, bottom - top)
            .ok()
            .filter(|r| r.area() > 0.0)?;
        let interpolated = self
            .keyframes
            .binary_search_by_key(&frame, |k| k.frame)
            .is_err();
        Some(ManualBox {
            bbox: clipped,
            interpolated,
        })
    }

    /// A track is applied only if it has a box in every one of its frames of a video of `frames`
    /// frames (1.12.1 `materializeManualVideoTrack` refuses the whole track otherwise).
    pub fn check(&self, frames: u64, size: (u32, u32)) -> Result<(), ManualTrackError> {
        self.ordered()?;
        if self.last >= frames {
            return Err(ManualTrackError::PastTheEnd);
        }
        match (self.first..=self.last).find(|&k| self.box_at(k, size).is_none()) {
            Some(frame) => Err(ManualTrackError::Outside(frame)),
            None => Ok(()),
        }
    }
}

/// A reviewer's changes to a video's tracks: tracks not to mask, and tracks drawn by hand.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct VideoEdits {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_tracks: Vec<TrackId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added_tracks: Vec<ManualTrack>,
}

impl VideoEdits {
    pub fn is_empty(&self) -> bool {
        self.excluded_tracks.is_empty() && self.added_tracks.is_empty()
    }

    pub fn check(
        &self,
        has_track: impl Fn(TrackId) -> bool,
        frames: u64,
        size: (u32, u32),
    ) -> Result<(), EditError> {
        if let Some(&id) = self.excluded_tracks.iter().find(|&&id| !has_track(id)) {
            return Err(EditError::NoSuchTrack(id));
        }
        for (track, manual) in self.added_tracks.iter().enumerate() {
            manual
                .check(frames, size)
                .map_err(|error| EditError::ManualTrack { track, error })?;
        }
        Ok(())
    }

    /// Whether a manual track shows in every frame of `first..=last`. Where in the frame cannot
    /// be checked: a gap has no box to compare with (M5 plan).
    pub fn covers(&self, first: FrameIndex, last: FrameIndex) -> bool {
        let mut spans: Vec<(FrameIndex, FrameIndex)> = self
            .added_tracks
            .iter()
            .map(|t| (t.first, t.last))
            .collect();
        spans.sort_unstable();
        let mut next = first;
        for (start, end) in spans {
            if start > next {
                break;
            }
            next = next.max(end.saturating_add(1));
            if next > last {
                return true;
            }
        }
        false
    }
}

/// A reviewer's changes to one item; only the half of the item's kind is ever set.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Edits {
    pub photo: PhotoEdits,
    pub video: VideoEdits,
}

impl std::error::Error for EditError {}

impl PhotoEdits {
    pub fn is_empty(&self) -> bool {
        self.excluded.is_empty() && self.added.is_empty()
    }

    pub fn check(&self, detections: usize) -> Result<(), EditError> {
        if let Some(&i) = self.excluded.iter().find(|&&i| i >= detections) {
            return Err(EditError::NoSuchDetection(i));
        }
        if let Some(i) = self.added.iter().position(|r| r.area() <= 0.0) {
            return Err(EditError::EmptyArea(i));
        }
        Ok(())
    }

    /// What to mask: every detection not excluded, then every added area.
    pub fn regions(&self, detections: &[Detection]) -> Vec<Region> {
        detections
            .iter()
            .enumerate()
            .filter(|(i, _)| !self.excluded.contains(i))
            .map(|(_, d)| Region::from(d))
            .chain(self.added.iter().map(|&bbox| Region::new(bbox)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detection::ModelId;

    fn key(input: u8) -> ReviewKey {
        ReviewKey {
            input: Digest([input; 32]),
            models: Digest([1; 32]),
            settings: Digest([2; 32]),
            timeline: Digest([3; 32]),
        }
    }

    #[test]
    fn rejects_stale_and_foreign_responses() {
        let first = ReviewRevision::first(key(0));
        let current = first.next();
        assert!(current.accepts(&current));
        assert!(!current.accepts(&first));
        assert!(!current.accepts(&ReviewRevision::first(key(9)).next()));
        assert_eq!(current.number(), 1);
    }

    fn rect(x: f32) -> Rect {
        Rect::new(x, 0.0, 10.0, 10.0).unwrap()
    }

    fn face(x: f32) -> Detection {
        Detection::new(ModelId::Yolo5FaceN, 0.9, rect(x), None, None).unwrap()
    }

    #[test]
    fn edits_drop_excluded_detections_and_add_areas() {
        let detections = [face(0.0), face(20.0), face(40.0)];
        let edits = PhotoEdits {
            excluded: vec![1],
            added: vec![rect(60.0)],
        };
        assert_eq!(edits.check(3), Ok(()));
        let boxes: Vec<f32> = edits
            .regions(&detections)
            .iter()
            .map(|r| r.bbox.x())
            .collect();
        assert_eq!(boxes, [0.0, 40.0, 60.0]);
        assert_eq!(PhotoEdits::default().regions(&detections).len(), 3);
    }

    fn manual(first: u64, last: u64, keyframes: &[(u64, f32)]) -> ManualTrack {
        ManualTrack {
            first,
            last,
            keyframes: keyframes
                .iter()
                .map(|&(frame, x)| Keyframe {
                    frame,
                    bbox: rect(x),
                })
                .collect(),
        }
    }

    #[test]
    fn a_manual_track_is_clipped_to_the_frame_and_refused_where_it_leaves_it() {
        let size = (100, 100);
        let edge = manual(0, 4, &[(0, 90.0), (4, 110.0)]);
        let at = |k| edge.box_at(k, size).map(|b| (b.bbox.x(), b.bbox.width()));
        assert_eq!(at(0), Some((90.0, 10.0)));
        assert_eq!(at(1), Some((95.0, 5.0)));
        assert_eq!(at(2), None);
        assert_eq!(edge.check(10, size), Err(ManualTrackError::Outside(2)));
        assert_eq!(edge.box_at(5, size), None);

        let inside = manual(2, 6, &[(3, 10.0)]);
        assert_eq!(inside.check(7, size), Ok(()));
        assert_eq!(inside.check(6, size), Err(ManualTrackError::PastTheEnd));
        assert!(inside.box_at(2, size).unwrap().interpolated);
        assert!(!inside.box_at(3, size).unwrap().interpolated);
        for bad in [
            manual(2, 6, &[]),
            manual(2, 6, &[(1, 0.0)]),
            manual(2, 6, &[(7, 0.0)]),
            manual(2, 6, &[(4, 0.0), (4, 5.0)]),
            manual(6, 2, &[(4, 0.0)]),
        ] {
            assert!(bad.check(10, size).is_err(), "{bad:?}");
            assert_eq!(bad.rect_at(4), None, "{bad:?}");
        }
    }

    #[test]
    fn manual_tracks_cover_a_gap_only_in_every_one_of_its_frames() {
        let edits = |spans: &[(u64, u64)]| VideoEdits {
            excluded_tracks: Vec::new(),
            added_tracks: spans
                .iter()
                .map(|&(first, last)| manual(first, last, &[(first, 0.0)]))
                .collect(),
        };
        assert!(edits(&[(30, 64)]).covers(30, 64));
        assert!(edits(&[(0, 100)]).covers(30, 64));
        assert!(edits(&[(40, 64), (25, 45)]).covers(30, 64));
        assert!(edits(&[(30, 40), (41, 64)]).covers(30, 64));
        assert!(!edits(&[(30, 40), (42, 64)]).covers(30, 64));
        assert!(!edits(&[(31, 64)]).covers(30, 64));
        assert!(!edits(&[(30, 63)]).covers(30, 64));
        assert!(!edits(&[]).covers(30, 64));
    }

    #[test]
    fn video_edits_name_existing_tracks() {
        let edits = VideoEdits {
            excluded_tracks: vec![1, 4],
            added_tracks: vec![manual(0, 3, &[(0, 0.0)])],
        };
        let known = |id| id <= 3;
        assert_eq!(
            edits.check(known, 10, (100, 100)),
            Err(EditError::NoSuchTrack(4))
        );
        let edits = VideoEdits {
            excluded_tracks: vec![1],
            ..edits
        };
        assert_eq!(edits.check(known, 10, (100, 100)), Ok(()));
        assert_eq!(
            edits.check(known, 3, (100, 100)),
            Err(EditError::ManualTrack {
                track: 0,
                error: ManualTrackError::PastTheEnd
            })
        );
    }

    #[test]
    fn edits_must_name_existing_detections_and_non_empty_areas() {
        let out_of_range = PhotoEdits {
            excluded: vec![0, 3],
            added: Vec::new(),
        };
        assert_eq!(out_of_range.check(3), Err(EditError::NoSuchDetection(3)));
        let empty = PhotoEdits {
            excluded: Vec::new(),
            added: vec![rect(0.0), Rect::new(5.0, 5.0, 0.0, 8.0).unwrap()],
        };
        assert_eq!(empty.check(0), Err(EditError::EmptyArea(1)));
    }
}
