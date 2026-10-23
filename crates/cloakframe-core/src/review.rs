use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::detection::Detection;
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
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchDetection(i) => write!(f, "the item has no detection #{i}"),
            Self::EmptyArea(i) => write!(f, "added area #{i} is empty"),
        }
    }
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
