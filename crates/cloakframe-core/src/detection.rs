use std::fmt;

use serde::{Deserialize, Serialize};

use crate::geometry::{Point, Rect};

pub type FrameIndex = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionKind {
    Face,
    Plate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelId {
    Yolo5FaceN,
    YuNet,
    PlateYolov9T,
}

impl ModelId {
    pub fn kind(self) -> DetectionKind {
        match self {
            Self::Yolo5FaceN | Self::YuNet => DetectionKind::Face,
            Self::PlateYolov9T => DetectionKind::Plate,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionError {
    ScoreOutOfRange,
    LandmarksOnPlate,
}

impl fmt::Display for DetectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ScoreOutOfRange => "score is outside 0..=1",
            Self::LandmarksOnPlate => "plate detections have no landmarks",
        })
    }
}

impl std::error::Error for DetectionError {}

/// One detector output in source display coordinates. The kind comes from the model, so face and
/// plate results keep separate thresholds and tracks (P-2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DetectionFields", into = "DetectionFields")]
pub struct Detection {
    model: ModelId,
    score: f32,
    bbox: Rect,
    landmarks: Option<[Point; 5]>,
    frame: Option<FrameIndex>,
}

#[derive(Serialize, Deserialize)]
struct DetectionFields {
    model: ModelId,
    score: f32,
    bbox: Rect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    landmarks: Option<[Point; 5]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    frame: Option<FrameIndex>,
}

impl TryFrom<DetectionFields> for Detection {
    type Error = DetectionError;

    fn try_from(f: DetectionFields) -> Result<Self, Self::Error> {
        Self::new(f.model, f.score, f.bbox, f.landmarks, f.frame)
    }
}

impl From<Detection> for DetectionFields {
    fn from(d: Detection) -> Self {
        Self {
            model: d.model,
            score: d.score,
            bbox: d.bbox,
            landmarks: d.landmarks,
            frame: d.frame,
        }
    }
}

impl Detection {
    pub fn new(
        model: ModelId,
        score: f32,
        bbox: Rect,
        landmarks: Option<[Point; 5]>,
        frame: Option<FrameIndex>,
    ) -> Result<Self, DetectionError> {
        if !(0.0..=1.0).contains(&score) {
            return Err(DetectionError::ScoreOutOfRange);
        }
        if landmarks.is_some() && model.kind() == DetectionKind::Plate {
            return Err(DetectionError::LandmarksOnPlate);
        }
        Ok(Self {
            model,
            score,
            bbox,
            landmarks,
            frame,
        })
    }

    pub fn kind(&self) -> DetectionKind {
        self.model.kind()
    }

    pub fn model(&self) -> ModelId {
        self.model
    }

    pub fn score(&self) -> f32 {
        self.score
    }

    pub fn bbox(&self) -> Rect {
        self.bbox
    }

    pub fn landmarks(&self) -> Option<&[Point; 5]> {
        self.landmarks.as_ref()
    }

    /// `None` for photos.
    pub fn frame(&self) -> Option<FrameIndex> {
        self.frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bbox() -> Rect {
        Rect::new(10.0, 20.0, 30.0, 40.0).unwrap()
    }

    #[test]
    fn kind_follows_the_model() {
        let face = Detection::new(ModelId::YuNet, 0.9, bbox(), None, Some(3)).unwrap();
        let plate = Detection::new(ModelId::PlateYolov9T, 0.9, bbox(), None, None).unwrap();
        assert_eq!(face.kind(), DetectionKind::Face);
        assert_eq!(plate.kind(), DetectionKind::Plate);
    }

    #[test]
    fn rejects_scores_outside_unit_range() {
        for score in [-0.01, 1.01, f32::NAN] {
            assert_eq!(
                Detection::new(ModelId::Yolo5FaceN, score, bbox(), None, None),
                Err(DetectionError::ScoreOutOfRange)
            );
        }
    }

    #[test]
    fn rejects_landmarks_on_plates() {
        let landmarks = [Point { x: 0.0, y: 0.0 }; 5];
        assert_eq!(
            Detection::new(ModelId::PlateYolov9T, 0.5, bbox(), Some(landmarks), None),
            Err(DetectionError::LandmarksOnPlate)
        );
        assert!(Detection::new(ModelId::YuNet, 0.5, bbox(), Some(landmarks), None).is_ok());
    }
}
