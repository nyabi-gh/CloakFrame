use serde::{Deserialize, Serialize};

use crate::detection::{DetectionKind, FrameIndex, ModelId};
use crate::geometry::Rect;

pub type TrackId = u32;

/// An automatic-detection problem that leaves part of the item possibly unmasked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Problem {
    /// The model returned more candidates after NMS than the per-frame limit kept (D11).
    CappedCandidates {
        model: ModelId,
        frame: Option<FrameIndex>,
        kept: u32,
        found: u32,
    },
    FailedFrame {
        frame: FrameIndex,
    },
    /// Subjects of one kind disappeared and came back without detections in between, for longer
    /// than tracking fills. Holes that touch are one gap; `tracks` are those either side of it,
    /// for review to show where to look.
    UnresolvedGap {
        detection: DetectionKind,
        tracks: Vec<TrackId>,
        first: FrameIndex,
        last: FrameIndex,
    },
    /// A candidate or track kept below the masking threshold.
    WeakCandidate {
        model: ModelId,
        frame: Option<FrameIndex>,
        bbox: Rect,
        score: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    Unresolved,
    CoveredByManualMask,
    /// The reviewer decided nothing in that area needs masking.
    Dismissed,
}

/// Opening or viewing the affected frames does not resolve a problem; only a manual mask or an
/// explicit dismissal does (1.12.1 treated "shown" as checked).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoverageEvidence {
    #[serde(flatten)]
    pub problem: Problem,
    pub resolution: Resolution,
}

impl CoverageEvidence {
    pub fn unresolved(problem: Problem) -> Self {
        Self {
            problem,
            resolution: Resolution::Unresolved,
        }
    }

    pub fn is_resolved(&self) -> bool {
        self.resolution != Resolution::Unresolved
    }
}

pub fn has_unresolved(evidence: &[CoverageEvidence]) -> bool {
    evidence.iter().any(|e| !e.is_resolved())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gap() -> CoverageEvidence {
        CoverageEvidence::unresolved(Problem::UnresolvedGap {
            detection: DetectionKind::Face,
            tracks: vec![1],
            first: 30,
            last: 64,
        })
    }

    #[test]
    fn every_problem_kind_starts_unresolved() {
        let bbox = Rect::new(0.0, 0.0, 8.0, 8.0).unwrap();
        let problems = [
            Problem::CappedCandidates {
                model: ModelId::Yolo5FaceN,
                frame: None,
                kept: 100,
                found: 259,
            },
            Problem::FailedFrame { frame: 7 },
            Problem::UnresolvedGap {
                detection: DetectionKind::Face,
                tracks: vec![1, 2],
                first: 30,
                last: 64,
            },
            Problem::WeakCandidate {
                model: ModelId::YuNet,
                frame: Some(3),
                bbox,
                score: 0.2,
            },
        ];
        for problem in problems {
            assert!(has_unresolved(&[CoverageEvidence::unresolved(problem)]));
        }
    }

    #[test]
    fn manual_mask_or_dismissal_resolves() {
        for resolution in [Resolution::CoveredByManualMask, Resolution::Dismissed] {
            let mut e = gap();
            e.resolution = resolution;
            assert!(!has_unresolved(&[e]));
        }
        assert!(has_unresolved(&[gap()]));
        assert!(!has_unresolved(&[]));
    }
}
