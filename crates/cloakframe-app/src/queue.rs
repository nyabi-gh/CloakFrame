//! The order a session's items are reviewed in (plan 6): unresolved problems first, then items
//! where nothing was detected, then the rest from the least confident detection up.

use std::cmp::Ordering;

use cloakframe_core::coverage::has_unresolved;
use cloakframe_engine::session::{SessionItem, StoredMedia};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Unresolved,
    NothingDetected,
    Detected,
}

pub fn group(item: &SessionItem) -> Group {
    if has_unresolved(&item.evidence) {
        Group::Unresolved
    } else if nothing_detected(&item.media) {
        Group::NothingDetected
    } else {
        Group::Detected
    }
}

fn nothing_detected(media: &StoredMedia) -> bool {
    match media {
        StoredMedia::Photo { detections, .. } => detections.is_empty(),
        StoredMedia::Video { tracks, .. } => tracks.is_empty(),
    }
}

/// The lowest confidence among what was detected: a photo's lowest score, and for a video 0 when
/// a track was kept below the masking threshold's confidence, 1 otherwise.
fn confidence(media: &StoredMedia) -> f32 {
    match media {
        StoredMedia::Photo { detections, .. } => detections
            .iter()
            .map(|d| d.score())
            .fold(f32::INFINITY, f32::min),
        StoredMedia::Video { tracks, .. } => {
            if tracks.iter().any(|t| t.low_confidence) {
                0.0
            } else {
                1.0
            }
        }
    }
}

/// Indices into `items` in review order; items of a group keep the session's order, which is
/// the scan's.
pub fn order(items: &[SessionItem]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(|&a, &b| {
        let (a, b) = (&items[a], &items[b]);
        group(a).cmp(&group(b)).then_with(|| {
            if group(a) == Group::Detected {
                confidence(&a.media)
                    .partial_cmp(&confidence(&b.media))
                    .unwrap_or(Ordering::Equal)
            } else {
                Ordering::Equal
            }
        })
    });
    order
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloakframe_core::coverage::{CoverageEvidence, Problem, Resolution};
    use cloakframe_core::detection::{Detection, ModelId};
    use cloakframe_core::geometry::Rect;
    use cloakframe_core::item::ItemId;
    use cloakframe_core::review::{Digest, ReviewKey, ReviewRevision};
    use cloakframe_engine::identity::SourceStamp;
    use std::path::PathBuf;

    fn face(score: f32) -> Detection {
        let bbox = Rect::new(0.0, 0.0, 10.0, 10.0).unwrap();
        Detection::new(ModelId::Yolo5FaceN, score, bbox, None, None).unwrap()
    }

    fn item(n: u64, scores: &[f32], problem: Option<Resolution>) -> SessionItem {
        let digest = Digest([0; 32]);
        let key = ReviewKey {
            input: digest,
            models: digest,
            settings: digest,
            timeline: digest,
        };
        let evidence = problem
            .map(|resolution| CoverageEvidence {
                problem: Problem::FailedFrame { frame: 0 },
                resolution,
            })
            .into_iter()
            .collect();
        SessionItem {
            item: ItemId(n),
            input: PathBuf::from(format!("{n}.jpg")),
            destination: PathBuf::from(format!("{n}.jpg")),
            conflict: None,
            suggestion: None,
            source: SourceStamp::of(std::path::Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/Cargo.toml"
            )))
            .unwrap(),
            revision: ReviewRevision::first(key),
            media: StoredMedia::Photo {
                detections: scores.iter().map(|&s| face(s)).collect(),
                edits: Default::default(),
            },
            evidence,
        }
    }

    #[test]
    fn unresolved_then_nothing_detected_then_least_confident() {
        let items = [
            item(0, &[0.9], None),
            item(1, &[], None),
            item(2, &[0.95, 0.55], None),
            item(3, &[0.9], Some(Resolution::Unresolved)),
            item(4, &[], Some(Resolution::Dismissed)),
            item(5, &[0.7], None),
            item(6, &[], Some(Resolution::Unresolved)),
        ];
        let ids: Vec<u64> = order(&items).iter().map(|&i| items[i].item.0).collect();
        assert_eq!(ids, [3, 6, 1, 4, 2, 5, 0]);
    }
}
