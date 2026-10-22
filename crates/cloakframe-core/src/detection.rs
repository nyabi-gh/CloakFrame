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

    /// The thresholds for this model's detections unless the user sets others. These are what
    /// 1.12.1 applied: score 0.5 and NMS 0.4, except that the plate graph suppressed at its own
    /// 0.45 and ignored the NMS setting. Revisited with the labeled benchmark (plan 9.2).
    pub fn default_thresholds(self) -> KindThresholds {
        let (score, nms_iou) = match self {
            Self::Yolo5FaceN | Self::YuNet => (0.5, 0.4),
            Self::PlateYolov9T => (0.5, 0.45),
        };
        KindThresholds { score, nms_iou }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionError {
    ScoreOutOfRange,
    LandmarksOnPlate,
    NonFiniteLandmark,
    /// A stored `kind` that is not the model's.
    KindMismatch,
}

impl fmt::Display for DetectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ScoreOutOfRange => "score is outside 0..=1",
            Self::LandmarksOnPlate => "plate detections have no landmarks",
            Self::NonFiniteLandmark => "landmark is not a finite point",
            Self::KindMismatch => "kind does not match the model",
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
    /// Derived from `model`; stored so results read on their own (P-2). Absent in the M1 form,
    /// which has the same session format version.
    #[serde(default)]
    kind: Option<DetectionKind>,
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
        if f.kind.is_some_and(|k| k != f.model.kind()) {
            return Err(DetectionError::KindMismatch);
        }
        Self::new(f.model, f.score, f.bbox, f.landmarks, f.frame)
    }
}

impl From<Detection> for DetectionFields {
    fn from(d: Detection) -> Self {
        Self {
            model: d.model,
            kind: Some(d.kind()),
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
        if let Some(points) = &landmarks {
            if model.kind() == DetectionKind::Plate {
                return Err(DetectionError::LandmarksOnPlate);
            }
            if !points.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
                return Err(DetectionError::NonFiniteLandmark);
            }
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

    /// In-plane face rotation in radians from the eye line, averaged with the mouth line when the
    /// two agree, as 1.12.1 `faceRollFromLandmarks` derives it. `None` when the landmarks are
    /// missing or implausible for the box; masks are then drawn upright.
    pub fn roll(&self) -> Option<f32> {
        let lm = self.landmarks.as_ref()?;
        let b = &self.bbox;
        if b.width() <= 0.0 || b.height() <= 0.0 {
            return None;
        }
        let (mx, my) = (b.width() * 0.35, b.height() * 0.35);
        let inside = |p: &Point| {
            p.x >= b.x() - mx
                && p.x <= b.right() + mx
                && p.y >= b.y() - my
                && p.y <= b.bottom() + my
        };
        if !lm.iter().all(inside) {
            return None;
        }
        let angle = |l: usize, r: usize| {
            let (dx, dy) = (lm[r].x - lm[l].x, lm[r].y - lm[l].y);
            let distance = dx.hypot(dy);
            if dx <= 0.0 || distance < b.width() * 0.08 || distance > b.width() * 1.25 {
                return None;
            }
            let a = dy.atan2(dx);
            (a.abs() <= MAX_ROLL).then_some(a)
        };
        let eyes = angle(0, 1)?;
        match angle(3, 4) {
            Some(mouth) if (mouth - eyes).abs() <= 0.45 => {
                Some((eyes.sin() + mouth.sin()).atan2(eyes.cos() + mouth.cos()))
            }
            _ => Some(eyes),
        }
    }
}

/// The largest roll the masks follow. 1.12.1's `kMaximumSupportedRoll` (1.0471975512F) is the
/// same f32.
pub const MAX_ROLL: f32 = std::f32::consts::FRAC_PI_3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThresholdError;

impl fmt::Display for ThresholdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("threshold is outside 0..=1")
    }
}

impl std::error::Error for ThresholdError {}

/// The minimum score a detection needs and the IoU above which NMS drops the lower-scored of two
/// boxes, for one kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KindThresholds {
    score: f32,
    nms_iou: f32,
}

impl KindThresholds {
    pub fn new(score: f32, nms_iou: f32) -> Result<Self, ThresholdError> {
        if !(0.0..=1.0).contains(&score) || !(0.0..=1.0).contains(&nms_iou) {
            return Err(ThresholdError);
        }
        Ok(Self { score, nms_iou })
    }

    pub fn score(&self) -> f32 {
        self.score
    }

    pub fn nms_iou(&self) -> f32 {
        self.nms_iou
    }
}

/// Separate thresholds per kind (P-2). 1.12.1 used one pair for faces and plates. The defaults
/// depend on the models (`ModelId::default_thresholds`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    pub face: KindThresholds,
    pub plate: KindThresholds,
}

impl Thresholds {
    pub fn of(&self, kind: DetectionKind) -> KindThresholds {
        match kind {
            DetectionKind::Face => self.face,
            DetectionKind::Plate => self.plate,
        }
    }
}

/// Drops detections below their kind's score threshold and empty boxes, then greedy NMS in
/// descending score order. Boxes of different kinds never suppress each other (P-2). Equal
/// scores keep their input order.
pub fn non_max_suppression(
    mut detections: Vec<Detection>,
    thresholds: &Thresholds,
) -> Vec<Detection> {
    detections.retain(|d| {
        d.score >= thresholds.of(d.kind()).score && d.bbox.width() > 0.0 && d.bbox.height() > 0.0
    });
    detections.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Detection> = Vec::with_capacity(detections.len());
    for d in detections {
        let limit = thresholds.of(d.kind()).nms_iou;
        if kept
            .iter()
            .all(|k| k.kind() != d.kind() || k.bbox.iou(&d.bbox) <= limit)
        {
            kept.push(d);
        }
    }
    kept
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

    fn face(score: f32, x: f32, y: f32, w: f32, h: f32) -> Detection {
        let bbox = Rect::new(x, y, w, h).unwrap();
        Detection::new(ModelId::Yolo5FaceN, score, bbox, None, None).unwrap()
    }

    fn plate(score: f32, x: f32, y: f32, w: f32, h: f32) -> Detection {
        let bbox = Rect::new(x, y, w, h).unwrap();
        Detection::new(ModelId::PlateYolov9T, score, bbox, None, None).unwrap()
    }

    fn all_kinds(score: f32, nms_iou: f32) -> Thresholds {
        let t = KindThresholds::new(score, nms_iou).unwrap();
        Thresholds { face: t, plate: t }
    }

    /// tests/fixtures/legacy/tests/testNonMaxSuppression.json (normative).
    #[test]
    fn legacy_non_max_suppression() {
        let input = vec![
            face(0.9, 0.0, 0.0, 10.0, 10.0),
            face(0.8, 1.0, 1.0, 10.0, 10.0),
            face(0.7, 100.0, 100.0, 10.0, 10.0),
        ];
        let kept = non_max_suppression(input.clone(), &all_kinds(0.0, 0.4));
        assert_eq!(kept, vec![input[0].clone(), input[2].clone()]);
    }

    /// The NMS call of tests/fixtures/legacy/tests/testInvalidDetectionsAreIgnored.json: NaN boxes
    /// and scores cannot be built at all, so only the valid box reaches NMS.
    #[test]
    fn legacy_invalid_detections_are_ignored() {
        assert!(Rect::new(f32::NAN, 0.0, 10.0, 10.0).is_err());
        let bbox = Rect::new(0.0, 0.0, 10.0, 10.0).unwrap();
        assert!(Detection::new(ModelId::Yolo5FaceN, f32::NAN, bbox, None, None).is_err());
        let valid = face(0.8, 2.0, 2.0, 8.0, 8.0);
        let empty = face(0.9, 0.0, 0.0, 0.0, 10.0);
        let kept = non_max_suppression(vec![empty, valid.clone()], &all_kinds(0.0, 0.4));
        assert_eq!(kept, vec![valid]);
    }

    #[test]
    fn nms_keeps_overlapping_boxes_of_different_kinds() {
        // IoU 0.6 between a face and a plate (defects/P-2 case 1).
        let a = face(0.9, 0.0, 0.0, 10.0, 10.0);
        let b = plate(0.8, 0.0, 2.5, 10.0, 10.0);
        assert!((a.bbox().iou(&b.bbox()) - 0.6).abs() < 1e-6);
        let kept = non_max_suppression(vec![a.clone(), b.clone()], &all_kinds(0.0, 0.4));
        assert_eq!(kept, vec![a.clone(), b]);
        let same_kind = face(0.8, 0.0, 2.5, 10.0, 10.0);
        assert_eq!(
            non_max_suppression(vec![a.clone(), same_kind], &all_kinds(0.0, 0.4)),
            vec![a]
        );
    }

    #[test]
    fn score_thresholds_apply_per_kind() {
        // defects/P-2 case 2.
        let thresholds = Thresholds {
            face: KindThresholds::new(0.5, 0.4).unwrap(),
            plate: KindThresholds::new(0.3, 0.4).unwrap(),
        };
        let weak_face = face(0.4, 0.0, 0.0, 10.0, 10.0);
        let weak_plate = plate(0.4, 50.0, 0.0, 10.0, 10.0);
        assert_eq!(
            non_max_suppression(vec![weak_face, weak_plate.clone()], &thresholds),
            vec![weak_plate]
        );
        assert_eq!(thresholds.of(DetectionKind::Plate).score(), 0.3);
    }

    #[test]
    fn thresholds_stay_in_unit_range() {
        for (score, nms) in [(-0.1, 0.4), (0.5, 1.1), (f32::NAN, 0.4)] {
            assert_eq!(KindThresholds::new(score, nms), Err(ThresholdError));
        }
    }

    #[test]
    fn defaults_are_what_1_12_1_applied() {
        let face = KindThresholds::new(0.5, 0.4).unwrap();
        assert_eq!(ModelId::Yolo5FaceN.default_thresholds(), face);
        assert_eq!(ModelId::YuNet.default_thresholds(), face);
        assert_eq!(
            ModelId::PlateYolov9T.default_thresholds(),
            KindThresholds::new(0.5, 0.45).unwrap()
        );
    }

    fn with_landmarks(points: [(f32, f32); 5]) -> Detection {
        let bbox = Rect::new(0.0, 0.0, 100.0, 100.0).unwrap();
        let lm = points.map(|(x, y)| Point { x, y });
        Detection::new(ModelId::Yolo5FaceN, 0.9, bbox, Some(lm), None).unwrap()
    }

    #[test]
    fn roll_follows_eyes_and_mouth() {
        let level = with_landmarks([
            (30.0, 40.0),
            (70.0, 40.0),
            (50.0, 55.0),
            (35.0, 75.0),
            (65.0, 75.0),
        ]);
        assert_eq!(level.roll(), Some(0.0));
        // Eyes at 45 degrees, mouth at 0: more than 0.45 rad apart, so the eyes alone.
        let tilted = with_landmarks([
            (30.0, 30.0),
            (60.0, 60.0),
            (50.0, 55.0),
            (35.0, 75.0),
            (65.0, 75.0),
        ]);
        assert!((tilted.roll().unwrap() - std::f32::consts::FRAC_PI_4).abs() < 1e-6);
        // Eyes and mouth close: their mean direction.
        let both = with_landmarks([
            (30.0, 40.0),
            (70.0, 44.0),
            (50.0, 55.0),
            (35.0, 75.0),
            (65.0, 78.0),
        ]);
        let (e, m) = ((4.0f32).atan2(40.0), (3.0f32).atan2(30.0));
        let mean = (e.sin() + m.sin()).atan2(e.cos() + m.cos());
        assert!((both.roll().unwrap() - mean).abs() < 1e-6);
        // A landmark far outside the box, eyes in the wrong order and no landmarks: no roll.
        let outside = with_landmarks([
            (30.0, 40.0),
            (70.0, 40.0),
            (50.0, 200.0),
            (35.0, 75.0),
            (65.0, 75.0),
        ]);
        let swapped = with_landmarks([
            (70.0, 40.0),
            (30.0, 40.0),
            (50.0, 55.0),
            (35.0, 75.0),
            (65.0, 75.0),
        ]);
        assert_eq!((outside.roll(), swapped.roll()), (None, None));
        assert_eq!(face(0.9, 0.0, 0.0, 10.0, 10.0).roll(), None);
    }

    #[test]
    fn rejects_non_finite_landmarks() {
        let bbox = Rect::new(0.0, 0.0, 10.0, 10.0).unwrap();
        let mut lm = [Point { x: 1.0, y: 1.0 }; 5];
        lm[2].y = f32::INFINITY;
        assert_eq!(
            Detection::new(ModelId::Yolo5FaceN, 0.5, bbox, Some(lm), None),
            Err(DetectionError::NonFiniteLandmark)
        );
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
