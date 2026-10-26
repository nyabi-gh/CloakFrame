//! A reviewer's work on one item until it is decided: which detections or tracks stay unmasked,
//! the areas or tracks added, how each problem is resolved, and the history undo and redo walk.

use cloakframe_core::coverage::{Problem, Resolution, TrackId};
use cloakframe_core::detection::FrameIndex;
use cloakframe_core::geometry::Rect;
use cloakframe_core::review::{Edits, Keyframe, ManualTrack, PhotoEdits, VideoEdits};
use cloakframe_engine::review::{Action, EvidenceResolution, ItemDecision};
use cloakframe_engine::session::{SessionItem, StoredMedia};

/// What undo restores.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Doc {
    pub edits: PhotoEdits,
    pub video: VideoEdits,
    /// One for each of the item's problems.
    pub resolutions: Vec<Resolution>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Detection(usize),
    Added(usize),
    Problem(usize),
    Track(TrackId),
    /// One of the manual tracks.
    Manual(usize),
}

/// What a video's edits are checked against.
#[derive(Debug, Clone, Default)]
struct Footage {
    tracks: Vec<TrackId>,
    frames: u64,
    /// Frames in a second, on average.
    rate: f64,
}

#[derive(Debug, Clone)]
pub struct Draft {
    pub doc: Doc,
    problems: Vec<Problem>,
    detections: usize,
    footage: Footage,
    /// The photo's or video's display size, which added areas stay inside, once it is known.
    pub bounds: Option<(f32, f32)>,
    undo: Vec<Doc>,
    redo: Vec<Doc>,
    /// The document before a drag that is still going, which undo takes back in one step.
    gesture: Option<Doc>,
    pub selected: Option<Pick>,
}

impl Draft {
    pub fn new(item: &SessionItem) -> Self {
        let (edits, video, detections, footage, bounds) = match &item.media {
            StoredMedia::Photo { detections, edits } => (
                edits.clone(),
                VideoEdits::default(),
                detections.len(),
                Footage::default(),
                None,
            ),
            StoredMedia::Video {
                frames,
                width,
                height,
                seconds,
                tracks,
                edits,
            } => (
                PhotoEdits::default(),
                edits.clone(),
                0,
                Footage {
                    tracks: tracks.iter().map(|t| t.id).collect(),
                    frames: *frames,
                    rate: if *seconds > 0.0 {
                        *frames as f64 / seconds
                    } else {
                        30.0
                    },
                },
                Some((*width as f32, *height as f32)),
            ),
        };
        Self {
            doc: Doc {
                edits,
                video,
                resolutions: item.evidence.iter().map(|e| e.resolution).collect(),
            },
            problems: item.evidence.iter().map(|e| e.problem.clone()).collect(),
            detections,
            footage,
            bounds,
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

    /// A manual-mask resolution holds only while the added areas or tracks still cover the
    /// problem, as the engine checks; the selection follows what still exists.
    fn settle(&mut self) {
        let Doc {
            edits,
            video,
            resolutions,
        } = &mut self.doc;
        for (resolution, problem) in resolutions.iter_mut().zip(&self.problems) {
            if *resolution == Resolution::CoveredByManualMask && !covers(edits, video, problem) {
                *resolution = Resolution::Unresolved;
            }
        }
        let exists = match self.selected {
            Some(Pick::Detection(i)) => i < self.detections,
            Some(Pick::Added(i)) => i < self.doc.edits.added.len(),
            Some(Pick::Problem(i)) => i < self.problems.len(),
            Some(Pick::Track(id)) => self.footage.tracks.contains(&id),
            Some(Pick::Manual(i)) => i < self.doc.video.added_tracks.len(),
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

    pub fn frames(&self) -> u64 {
        self.footage.frames
    }

    /// Frames in a second, to the nearest, at least one.
    pub fn second(&self) -> u64 {
        (self.footage.rate.round() as u64).max(1)
    }

    pub fn is_track_excluded(&self, track: TrackId) -> bool {
        self.doc.video.excluded_tracks.contains(&track)
    }

    pub fn toggle_track(&mut self, track: TrackId) {
        if !self.footage.tracks.contains(&track) {
            return;
        }
        self.change(|doc, _| {
            let excluded = &mut doc.video.excluded_tracks;
            match excluded.iter().position(|&t| t == track) {
                Some(at) => {
                    excluded.remove(at);
                }
                None => {
                    excluded.push(track);
                    excluded.sort_unstable();
                }
            }
        });
    }

    /// Draws a track from frame `k` for a second (to the video's end at most), its box `area` cut
    /// to the frame at its one keyframe.
    pub fn add_track(&mut self, k: FrameIndex, area: Rect) -> Option<usize> {
        let area = clamp(area, self.bounds)?;
        if k >= self.footage.frames {
            return None;
        }
        let last = (k + self.second() - 1).min(self.footage.frames - 1);
        self.change(|doc, _| {
            doc.video.added_tracks.push(ManualTrack {
                first: k,
                last,
                keyframes: vec![Keyframe {
                    frame: k,
                    bbox: area,
                }],
            });
        });
        let index = self.doc.video.added_tracks.len() - 1;
        self.selected = Some(Pick::Manual(index));
        Some(index)
    }

    /// Sets manual track `track`'s box at frame `k`, in its frames, as a keyframe: `change` turns
    /// the box it shows there now into the new one, cut to the frame.
    pub fn set_box(
        doc: &mut Doc,
        track: usize,
        k: FrameIndex,
        change: impl FnOnce(Rect) -> Option<Rect>,
        bounds: Option<(f32, f32)>,
    ) {
        let Some(manual) = doc.video.added_tracks.get_mut(track) else {
            return;
        };
        let Some(now) = manual.rect_at(k) else {
            return;
        };
        let Some(bbox) = change(now).and_then(|r| clamp(r, bounds)) else {
            return;
        };
        let keyframe = Keyframe { frame: k, bbox };
        match manual.keyframes.binary_search_by_key(&k, |f| f.frame) {
            Ok(at) => manual.keyframes[at] = keyframe,
            Err(at) => manual.keyframes.insert(at, keyframe),
        }
    }

    /// Moves a manual track's box at `k` by `(dx, dy)`, keeping it inside the frame.
    pub fn shift_track(
        doc: &mut Doc,
        track: usize,
        k: FrameIndex,
        (dx, dy): (f32, f32),
        bounds: Option<(f32, f32)>,
    ) {
        Self::set_box(
            doc,
            track,
            k,
            |r| {
                let (mut x, mut y) = (r.x() + dx, r.y() + dy);
                if let Some((w, h)) = bounds {
                    x = x.clamp(0.0, (w - r.width()).max(0.0));
                    y = y.clamp(0.0, (h - r.height()).max(0.0));
                }
                Rect::new(x, y, r.width(), r.height()).ok()
            },
            bounds,
        );
    }

    /// Grows or shrinks a manual track's box at `k` from its right and bottom edges.
    pub fn grow_track(
        doc: &mut Doc,
        track: usize,
        k: FrameIndex,
        (dw, dh): (f32, f32),
        bounds: Option<(f32, f32)>,
    ) {
        Self::set_box(
            doc,
            track,
            k,
            |r| {
                let width = (r.width() + dw).max(1.0);
                let height = (r.height() + dh).max(1.0);
                Rect::new(r.x(), r.y(), width, height).ok()
            },
            bounds,
        );
    }

    /// Makes `k` the first frame of manual track `track` (`start`) or its last. The box it showed
    /// nearest `k` becomes a keyframe there, and keyframes left outside go.
    pub fn trim(&mut self, track: usize, k: FrameIndex, start: bool) {
        if k >= self.footage.frames {
            return;
        }
        self.change(|doc, _| {
            let Some(manual) = doc.video.added_tracks.get_mut(track) else {
                return;
            };
            let held = manual.rect_at(k.clamp(manual.first, manual.last));
            let Some(bbox) = held else {
                return;
            };
            if start {
                manual.first = k;
                manual.last = manual.last.max(k);
            } else {
                manual.last = k;
                manual.first = manual.first.min(k);
            }
            let (first, last) = (manual.first, manual.last);
            manual
                .keyframes
                .retain(|f| f.frame >= first && f.frame <= last);
            if let Err(at) = manual.keyframes.binary_search_by_key(&k, |f| f.frame) {
                manual.keyframes.insert(at, Keyframe { frame: k, bbox });
            }
        });
    }

    /// Removes manual track `track`'s keyframe at `k`, unless it is the track's only one.
    pub fn remove_keyframe(&mut self, track: usize, k: FrameIndex) {
        self.change(|doc, _| {
            if let Some(manual) = doc.video.added_tracks.get_mut(track)
                && manual.keyframes.len() > 1
                && let Ok(at) = manual.keyframes.binary_search_by_key(&k, |f| f.frame)
            {
                manual.keyframes.remove(at);
            }
        });
    }

    pub fn remove_track(&mut self, track: usize) {
        self.change(|doc, _| {
            if track < doc.video.added_tracks.len() {
                doc.video.added_tracks.remove(track);
            }
        });
    }

    /// Draws a manual track over every frame of gap `problem` and marks the gap covered, as one
    /// step. Its keyframes are `before`, the box of a track the frame before the gap, at the gap's
    /// first frame and `after`, one the frame after, at its last; with neither, a box in the
    /// middle of the frame for the reviewer to place.
    pub fn cover_gap(&mut self, problem: usize, before: Option<Rect>, after: Option<Rect>) {
        let Some(&Problem::UnresolvedGap { first, last, .. }) = self.problems.get(problem) else {
            return;
        };
        if first > last || last >= self.footage.frames {
            return;
        }
        let middle = self.bounds.and_then(|(w, h)| {
            let side = (w.min(h) / 5.0).max(2.0);
            Rect::new((w - side) / 2.0, (h - side) / 2.0, side, side).ok()
        });
        let bounds = self.bounds;
        let keyframe = |frame, bbox: Option<Rect>| {
            bbox.and_then(|b| clamp(b, bounds))
                .map(|bbox| Keyframe { frame, bbox })
        };
        let mut keyframes: Vec<Keyframe> = [keyframe(first, before), keyframe(last, after)]
            .into_iter()
            .flatten()
            .collect();
        keyframes.dedup_by_key(|k| k.frame);
        if keyframes.is_empty() {
            keyframes.extend(keyframe(first, middle));
        }
        if keyframes.is_empty() {
            return;
        }
        self.change(|doc, _| {
            doc.video.added_tracks.push(ManualTrack {
                first,
                last,
                keyframes,
            });
            if let Some(r) = doc.resolutions.get_mut(problem) {
                *r = Resolution::CoveredByManualMask;
            }
        });
        self.selected = Some(Pick::Manual(self.doc.video.added_tracks.len() - 1));
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
            StoredMedia::Video { .. } => Edits {
                video: self.doc.video.clone(),
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
fn covers(photo: &PhotoEdits, video: &VideoEdits, problem: &Problem) -> bool {
    match problem {
        Problem::WeakCandidate { bbox, .. } => bbox.is_covered_by(&photo.added),
        Problem::UnresolvedGap { first, last, .. } => video.covers(*first, *last),
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

    fn video_item() -> SessionItem {
        let summary = |id, first, last| cloakframe_engine::video::tracking::TrackSummary {
            id,
            kind: cloakframe_core::detection::DetectionKind::Face,
            first,
            last,
            detections: 10,
            strong: 10,
            low_confidence: false,
        };
        SessionItem {
            media: StoredMedia::Video {
                frames: 300,
                width: 640,
                height: 360,
                seconds: 10.0,
                tracks: vec![summary(1, 0, 29), summary(2, 90, 200)],
                edits: VideoEdits::default(),
            },
            evidence: vec![CoverageEvidence::unresolved(Problem::UnresolvedGap {
                detection: cloakframe_core::detection::DetectionKind::Face,
                tracks: vec![1, 2],
                first: 30,
                last: 89,
            })],
            ..item()
        }
    }

    #[test]
    fn a_drawn_track_runs_a_second_and_takes_a_keyframe_where_its_box_changes() {
        let stored = video_item();
        let mut draft = Draft::new(&stored);
        assert_eq!((draft.frames(), draft.second()), (300, 30));
        let at = draft.add_track(280, rect(600.0, 0.0, 80.0, 40.0)).unwrap();
        let track = &draft.doc.video.added_tracks[at];
        assert_eq!(
            (track.first, track.last),
            (280, 299),
            "cut at the video's end"
        );
        assert_eq!(
            track.keyframes[0].bbox,
            rect(600.0, 0.0, 40.0, 40.0),
            "cut to the frame"
        );
        assert_eq!(draft.selected, Some(Pick::Manual(0)));
        draft.remove_track(0);
        assert_eq!(draft.selected, None);

        draft.add_track(10, rect(0.0, 0.0, 10.0, 10.0)).unwrap();
        assert_eq!(draft.doc.video.added_tracks[0].last, 39);
        draft.begin_gesture();
        for _ in 0..4 {
            draft.gesture(|doc, b| Draft::shift_track(doc, 0, 30, (5.0, 0.0), b));
        }
        draft.end_gesture();
        let frames: Vec<u64> = draft.doc.video.added_tracks[0]
            .keyframes
            .iter()
            .map(|k| k.frame)
            .collect();
        assert_eq!(frames, [10, 30]);
        let track = &draft.doc.video.added_tracks[0];
        assert_eq!(track.rect_at(30), Some(rect(20.0, 0.0, 10.0, 10.0)));
        assert_eq!(track.rect_at(20), Some(rect(10.0, 0.0, 10.0, 10.0)));
        draft.undo();
        assert_eq!(
            draft.doc.video.added_tracks[0].keyframes.len(),
            1,
            "the drag is one step"
        );
        draft.redo();
        draft.change(|doc, b| Draft::shift_track(doc, 0, 50, (5.0, 0.0), b));
        assert_eq!(
            draft.doc.video.added_tracks[0].keyframes.len(),
            2,
            "nothing is set outside the track"
        );
        draft.change(|doc, b| Draft::grow_track(doc, 0, 30, (-20.0, 1000.0), b));
        assert_eq!(
            draft.doc.video.added_tracks[0].rect_at(30),
            Some(rect(20.0, 0.0, 1.0, 360.0))
        );

        draft.trim(0, 20, true);
        let track = &draft.doc.video.added_tracks[0];
        assert_eq!((track.first, track.last), (20, 39));
        let frames: Vec<u64> = track.keyframes.iter().map(|k| k.frame).collect();
        assert_eq!(frames, [20, 30], "the box at the new start is kept");
        draft.trim(0, 60, false);
        assert_eq!(draft.doc.video.added_tracks[0].last, 60);
        draft.trim(0, 25, false);
        let track = &draft.doc.video.added_tracks[0];
        assert_eq!((track.first, track.last), (20, 25));
        let frames: Vec<u64> = track.keyframes.iter().map(|k| k.frame).collect();
        assert_eq!(frames, [20, 25]);
        draft.remove_keyframe(0, 25);
        draft.remove_keyframe(0, 20);
        assert_eq!(
            draft.doc.video.added_tracks[0].keyframes.len(),
            1,
            "the last keyframe stays"
        );
        assert!(
            draft
                .doc
                .video
                .check(|id| id == 1 || id == 2, 300, (640, 360))
                .is_ok()
        );
    }

    #[test]
    fn a_gap_is_covered_only_while_drawn_tracks_show_in_all_its_frames() {
        let stored = video_item();
        let mut draft = Draft::new(&stored);
        draft.toggle_track(2);
        draft.toggle_track(9);
        assert!(draft.is_track_excluded(2) && !draft.is_track_excluded(9));
        draft.cover_gap(
            0,
            Some(rect(100.0, 100.0, 40.0, 40.0)),
            Some(rect(300.0, 100.0, 40.0, 40.0)),
        );
        assert_eq!(draft.unresolved(), 0);
        let track = &draft.doc.video.added_tracks[0];
        assert_eq!((track.first, track.last), (30, 89));
        assert_eq!(track.rect_at(30), Some(rect(100.0, 100.0, 40.0, 40.0)));
        assert_eq!(track.rect_at(89), Some(rect(300.0, 100.0, 40.0, 40.0)));
        draft.trim(0, 31, true);
        assert_eq!(draft.unresolved(), 1, "frame 30 is no longer covered");
        draft.undo();
        assert_eq!(draft.unresolved(), 0);

        let decision = draft.decision(&stored, Action::Approve, None);
        assert_eq!(decision.edits.video, draft.doc.video);
        assert!(decision.edits.photo.is_empty());
        assert_eq!(
            decision.resolutions,
            [EvidenceResolution {
                evidence: 0,
                resolution: Resolution::CoveredByManualMask
            }]
        );

        let mut middle = Draft::new(&stored);
        middle.cover_gap(0, None, None);
        let track = &middle.doc.video.added_tracks[0];
        assert_eq!(track.keyframes.len(), 1);
        assert_eq!(track.keyframes[0].bbox, rect(284.0, 144.0, 72.0, 72.0));
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
