//! A reviewer's work on one item until it is decided: which detections stay unmasked, the areas
//! added, how each problem is resolved, and the history undo and redo walk.

use cloakframe_core::coverage::{Problem, Resolution};
use cloakframe_core::geometry::Rect;
use cloakframe_core::review::{Edits, PhotoEdits};
use cloakframe_engine::review::{Action, EvidenceResolution, ItemDecision};
use cloakframe_engine::session::{SessionItem, StoredMedia};

/// What undo restores.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Doc {
    pub edits: PhotoEdits,
    /// One for each of the item's problems.
    pub resolutions: Vec<Resolution>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Detection(usize),
    Added(usize),
    Problem(usize),
}

#[derive(Debug, Clone)]
pub struct Draft {
    pub doc: Doc,
    problems: Vec<Problem>,
    detections: usize,
    /// The photo's display size, which added areas stay inside, once it is known.
    pub bounds: Option<(f32, f32)>,
    undo: Vec<Doc>,
    redo: Vec<Doc>,
    /// The document before a drag that is still going, which undo takes back in one step.
    gesture: Option<Doc>,
    pub selected: Option<Pick>,
}

impl Draft {
    pub fn new(item: &SessionItem) -> Self {
        let (edits, detections) = match &item.media {
            StoredMedia::Photo { detections, edits } => (edits.clone(), detections.len()),
            StoredMedia::Video { .. } => (PhotoEdits::default(), 0),
        };
        Self {
            doc: Doc {
                edits,
                resolutions: item.evidence.iter().map(|e| e.resolution).collect(),
            },
            problems: item.evidence.iter().map(|e| e.problem.clone()).collect(),
            detections,
            bounds: None,
            undo: Vec::new(),
            redo: Vec::new(),
            gesture: None,
            selected: None,
        }
    }

    pub fn unresolved(&self) -> usize {
        self.doc
            .resolutions
            .iter()
            .filter(|&&r| r == Resolution::Unresolved)
            .count()
    }

    pub fn is_excluded(&self, detection: usize) -> bool {
        self.doc.edits.excluded.contains(&detection)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Changes the document as one step of the history.
    pub fn change(&mut self, f: impl FnOnce(&mut Doc, Option<(f32, f32)>)) {
        let before = self.doc.clone();
        f(&mut self.doc, self.bounds);
        self.settle();
        if self.doc != before {
            self.undo.push(before);
            self.redo.clear();
        }
    }

    /// Starts a drag whose every change is one step of the history.
    pub fn begin_gesture(&mut self) {
        self.gesture = Some(self.doc.clone());
    }

    pub fn gesture(&mut self, f: impl FnOnce(&mut Doc, Option<(f32, f32)>)) {
        f(&mut self.doc, self.bounds);
        self.settle();
    }

    pub fn end_gesture(&mut self) {
        if let Some(before) = self.gesture.take()
            && self.doc != before
        {
            self.undo.push(before);
            self.redo.clear();
        }
    }

    pub fn undo(&mut self) {
        self.end_gesture();
        if let Some(before) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.doc, before));
            self.settle();
        }
    }

    pub fn redo(&mut self) {
        self.end_gesture();
        if let Some(after) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.doc, after));
            self.settle();
        }
    }

    /// A manual-mask resolution holds only while the added areas still cover the problem's box,
    /// as the engine checks; the selection follows what still exists.
    fn settle(&mut self) {
        for (resolution, problem) in self.doc.resolutions.iter_mut().zip(&self.problems) {
            if *resolution == Resolution::CoveredByManualMask
                && !covers(&self.doc.edits.added, problem)
            {
                *resolution = Resolution::Unresolved;
            }
        }
        let exists = match self.selected {
            Some(Pick::Detection(i)) => i < self.detections,
            Some(Pick::Added(i)) => i < self.doc.edits.added.len(),
            Some(Pick::Problem(i)) => i < self.problems.len(),
            None => true,
        };
        if !exists {
            self.selected = None;
        }
    }

    pub fn toggle(&mut self, detection: usize) {
        if detection >= self.detections {
            return;
        }
        self.change(|doc, _| {
            let excluded = &mut doc.edits.excluded;
            match excluded.iter().position(|&i| i == detection) {
                Some(at) => {
                    excluded.remove(at);
                }
                None => {
                    excluded.push(detection);
                    excluded.sort_unstable();
                }
            }
        });
    }

    /// Adds an area, cut to the photo; nothing is added where nothing is left of it.
    pub fn add(&mut self, area: Rect) -> Option<usize> {
        let area = clamp(area, self.bounds)?;
        self.change(|doc, _| doc.edits.added.push(area));
        let index = self.doc.edits.added.len() - 1;
        self.selected = Some(Pick::Added(index));
        Some(index)
    }

    pub fn remove(&mut self, added: usize) {
        self.change(|doc, _| {
            if added < doc.edits.added.len() {
                doc.edits.added.remove(added);
            }
        });
    }

    /// Moves an added area, keeping it inside the photo.
    pub fn shift(doc: &mut Doc, added: usize, (dx, dy): (f32, f32), bounds: Option<(f32, f32)>) {
        let Some(area) = doc.edits.added.get_mut(added) else {
            return;
        };
        let (mut x, mut y) = (area.x() + dx, area.y() + dy);
        if let Some((w, h)) = bounds {
            x = x.clamp(0.0, (w - area.width()).max(0.0));
            y = y.clamp(0.0, (h - area.height()).max(0.0));
        }
        if let Ok(moved) = Rect::new(x, y, area.width(), area.height()) {
            *area = moved;
        }
    }

    /// Grows or shrinks an added area from its right and bottom edges, keeping at least a pixel
    /// and staying inside the photo.
    pub fn grow(doc: &mut Doc, added: usize, (dw, dh): (f32, f32), bounds: Option<(f32, f32)>) {
        let Some(area) = doc.edits.added.get_mut(added) else {
            return;
        };
        let (max_w, max_h) = bounds.map_or((f32::INFINITY, f32::INFINITY), |(w, h)| {
            (w - area.x(), h - area.y())
        });
        let width = (area.width() + dw).clamp(1.0, max_w.max(1.0));
        let height = (area.height() + dh).clamp(1.0, max_h.max(1.0));
        if let Ok(resized) = Rect::new(area.x(), area.y(), width, height) {
            *area = resized;
        }
    }

    pub fn resolve(&mut self, problem: usize, resolution: Resolution) {
        self.change(|doc, _| {
            if let Some(r) = doc.resolutions.get_mut(problem) {
                *r = resolution;
            }
        });
    }

    /// Masks a weak candidate's box and marks it covered, as one step.
    pub fn cover(&mut self, problem: usize) {
        let Some(Problem::WeakCandidate { bbox, .. }) = self.problems.get(problem) else {
            return;
        };
        let bbox = *bbox;
        let Some(area) = clamp(bbox, self.bounds) else {
            return;
        };
        self.change(|doc, _| {
            if !bbox.is_covered_by(&doc.edits.added) {
                doc.edits.added.push(area);
            }
            if let Some(r) = doc.resolutions.get_mut(problem) {
                *r = Resolution::CoveredByManualMask;
            }
        });
    }

    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    /// The decision for `item` as it now is, bound to the revision and key it was made on.
    pub fn decision(
        &self,
        item: &SessionItem,
        action: Action,
        rename_to: Option<String>,
    ) -> ItemDecision {
        let edits = match &item.media {
            StoredMedia::Photo { .. } => Edits {
                photo: self.doc.edits.clone(),
                ..Edits::default()
            },
            // Video edits come in stage 3; the stored ones stay.
            StoredMedia::Video { edits, .. } => Edits {
                video: edits.clone(),
                ..Edits::default()
            },
        };
        ItemDecision {
            item: item.item,
            revision: item.revision.number(),
            action,
            resolutions: self
                .doc
                .resolutions
                .iter()
                .enumerate()
                .filter(|(_, r)| **r != Resolution::Unresolved)
                .map(|(evidence, &resolution)| EvidenceResolution {
                    evidence,
                    resolution,
                })
                .collect(),
            edits,
            rename_to,
            key: Some(item.revision.key()),
        }
    }
}

/// Whether a manual mask can resolve the problem, and does.
fn covers(added: &[Rect], problem: &Problem) -> bool {
    match problem {
        Problem::WeakCandidate { bbox, .. } => bbox.is_covered_by(added),
        _ => false,
    }
}

/// `area` cut to the photo, `None` if nothing of it is left.
pub fn clamp(area: Rect, bounds: Option<(f32, f32)>) -> Option<Rect> {
    let Some((w, h)) = bounds else {
        return Some(area).filter(|a| a.area() > 0.0);
    };
    let left = area.x().max(0.0);
    let top = area.y().max(0.0);
    let right = area.right().min(w);
    let bottom = area.bottom().min(h);
    Rect::new(left, top, right - left, bottom - top)
        .ok()
        .filter(|a| a.area() > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloakframe_core::coverage::CoverageEvidence;
    use cloakframe_core::detection::{Detection, ModelId};
    use cloakframe_core::item::ItemId;
    use cloakframe_core::review::{Digest, ReviewKey, ReviewRevision};
    use cloakframe_engine::identity::SourceStamp;
    use std::path::{Path, PathBuf};

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(x, y, w, h).unwrap()
    }

    fn item() -> SessionItem {
        let key = ReviewKey {
            input: Digest([1; 32]),
            models: Digest([2; 32]),
            settings: Digest([3; 32]),
            timeline: Digest([4; 32]),
        };
        let face = |x| {
            Detection::new(
                ModelId::Yolo5FaceN,
                0.9,
                rect(x, 0.0, 10.0, 10.0),
                None,
                None,
            )
            .unwrap()
        };
        SessionItem {
            item: ItemId(7),
            input: PathBuf::from("a.jpg"),
            destination: PathBuf::from("a.jpg"),
            conflict: None,
            suggestion: None,
            source: SourceStamp::of(Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/Cargo.toml"
            )))
            .unwrap(),
            revision: ReviewRevision::first(key).next(),
            media: StoredMedia::Photo {
                detections: vec![face(0.0), face(20.0)],
                edits: PhotoEdits::default(),
            },
            evidence: vec![
                CoverageEvidence::unresolved(Problem::WeakCandidate {
                    model: ModelId::Yolo5FaceN,
                    frame: None,
                    bbox: rect(50.0, 50.0, 8.0, 8.0),
                    score: 0.3,
                }),
                CoverageEvidence::unresolved(Problem::CappedCandidates {
                    model: ModelId::Yolo5FaceN,
                    frame: None,
                    kept: 1,
                    found: 3,
                }),
            ],
        }
    }

    #[test]
    fn every_change_is_undone_and_redone_in_order() {
        let mut draft = Draft::new(&item());
        draft.bounds = Some((100.0, 80.0));
        draft.toggle(1);
        assert!(draft.is_excluded(1));
        let added = draft.add(rect(90.0, 70.0, 20.0, 20.0)).unwrap();
        assert_eq!(
            draft.doc.edits.added[added],
            rect(90.0, 70.0, 10.0, 10.0),
            "cut to the photo"
        );
        assert_eq!(draft.selected, Some(Pick::Added(0)));
        draft.begin_gesture();
        for _ in 0..5 {
            draft.gesture(|doc, bounds| Draft::shift(doc, 0, (-2.0, -1.0), bounds));
        }
        draft.end_gesture();
        assert_eq!(draft.doc.edits.added[0], rect(80.0, 65.0, 10.0, 10.0));
        draft.change(|doc, bounds| Draft::shift(doc, 0, (50.0, 50.0), bounds));
        assert_eq!(
            draft.doc.edits.added[0],
            rect(90.0, 70.0, 10.0, 10.0),
            "kept inside"
        );
        draft.change(|doc, bounds| Draft::grow(doc, 0, (-30.0, 5.0), bounds));
        assert_eq!(draft.doc.edits.added[0], rect(90.0, 70.0, 1.0, 10.0));

        draft.undo();
        draft.undo();
        assert_eq!(draft.doc.edits.added[0], rect(80.0, 65.0, 10.0, 10.0));
        draft.undo();
        assert_eq!(
            draft.doc.edits.added[0],
            rect(90.0, 70.0, 10.0, 10.0),
            "the drag is one step"
        );
        draft.undo();
        assert!(draft.doc.edits.added.is_empty());
        assert_eq!(draft.selected, None, "the selected area is gone");
        draft.undo();
        assert!(!draft.is_excluded(1) && !draft.can_undo());
        draft.redo();
        draft.redo();
        assert!(draft.is_excluded(1));
        assert_eq!(draft.doc.edits.added.len(), 1);
        draft.toggle(0);
        assert!(!draft.can_redo(), "a new change drops what was undone");
    }

    #[test]
    fn a_covered_problem_stays_resolved_only_while_its_box_is_masked() {
        let mut draft = Draft::new(&item());
        draft.bounds = Some((100.0, 80.0));
        assert_eq!(draft.unresolved(), 2);
        draft.cover(1);
        assert_eq!(
            draft.unresolved(),
            2,
            "a capped candidate has no box to cover"
        );
        draft.cover(0);
        assert_eq!(draft.doc.resolutions[0], Resolution::CoveredByManualMask);
        assert_eq!(draft.doc.edits.added, [rect(50.0, 50.0, 8.0, 8.0)]);
        draft.resolve(1, Resolution::Dismissed);
        assert_eq!(draft.unresolved(), 0);
        draft.change(|doc, bounds| Draft::shift(doc, 0, (4.0, 0.0), bounds));
        assert_eq!(draft.doc.resolutions[0], Resolution::Unresolved);
        draft.undo();
        assert_eq!(draft.doc.resolutions[0], Resolution::CoveredByManualMask);
        draft.remove(0);
        assert_eq!(draft.unresolved(), 1);
    }

    #[test]
    fn the_decision_carries_the_document_revision_and_key() {
        let stored = item();
        let mut draft = Draft::new(&stored);
        draft.toggle(0);
        draft.resolve(1, Resolution::Dismissed);
        let decision = draft.decision(&stored, Action::ExportWithWarnings, None);
        assert_eq!(decision.revision, 1);
        assert_eq!(decision.key, Some(stored.revision.key()));
        assert_eq!(decision.edits.photo.excluded, [0]);
        assert_eq!(
            decision.resolutions,
            [EvidenceResolution {
                evidence: 1,
                resolution: Resolution::Dismissed
            }]
        );
    }
}
