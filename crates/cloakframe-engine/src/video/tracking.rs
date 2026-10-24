//! Tracking a video from its record (plan 4.4, docs/M5_TRACK.md): each kind on its own trackers
//! (P-2), backwards and then forwards, the passes joined, and the tracks post-processed into the
//! regions to mask in every frame. The record is read once backwards and three times forwards,
//! so memory holds the open tracks and a summary of each track, not the video's detections.
//!
//! 1. Backward tracking, its assignments appended to a scratch record from the last frame down.
//! 2. Forward tracking with the backward assignments read back in frame order: the join.
//! 3. The same, giving each track's boxes for the summaries post-processing needs up front.
//! 4. The same again into post-processing, which writes the regions.
//!
//! Tracking is deterministic, so the forward passes find the same tracks each time. Tracks are
//! numbered by kind in the models' order: the first kind's from 1, the next kind's after them.

use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::path::Path;

use cloakframe_core::coverage::{CoverageEvidence, Problem};
use cloakframe_core::detection::{DetectionKind, FrameIndex};
use cloakframe_core::timeline::Rational;
use cloakframe_core::tracking::merge::{Joins, MERGE_IOU, Merger};
use cloakframe_core::tracking::scene::SceneCuts;
use cloakframe_core::tracking::stream::{Frame, PostProcessor, Summaries};
use cloakframe_core::tracking::tracker::{Assignment, ByteTracker};
use cloakframe_core::tracking::{
    Observation, PostProcessConfig, TrackId, TrackedBox, TrackerConfig, TrackingWindows,
};

use serde::{Deserialize, Serialize};

use super::analysis::DETECTION_FLOOR;
use super::record::{Assigned, Detected, Order, Record, RecordError, RecordWriter, TrackRegion};
use crate::cancel::CancelToken;

#[derive(Debug)]
pub enum TrackingError {
    Record(RecordError),
    Cancelled,
}

impl fmt::Display for TrackingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Record(e) => e.fmt(f),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for TrackingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Record(e) => Some(e),
            Self::Cancelled => None,
        }
    }
}

impl From<RecordError> for TrackingError {
    fn from(e: RecordError) -> Self {
        Self::Record(e)
    }
}

impl From<io::Error> for TrackingError {
    fn from(e: io::Error) -> Self {
        Self::Record(RecordError::Io(e))
    }
}

/// How one kind is tracked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KindTracking {
    pub kind: DetectionKind,
    pub tracker: TrackerConfig,
    pub post: PostProcessConfig,
}

impl KindTracking {
    /// `windows` at the source's frame rate, with `score` starting tracks and counting as strong.
    pub fn new(kind: DetectionKind, score: f32, rate: Rational, windows: &TrackingWindows) -> Self {
        let (mut tracker, mut post) = (TrackerConfig::default(), PostProcessConfig::default());
        windows.apply(rate, &mut tracker, &mut post);
        tracker.high_score = score;
        tracker.low_score = DETECTION_FLOOR;
        post.strong_score = score;
        Self {
            kind,
            tracker,
            post,
        }
    }
}

/// A kept track, for review and the results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackSummary {
    pub id: TrackId,
    pub kind: DetectionKind,
    /// The frames of its first and last detection.
    pub first: FrameIndex,
    pub last: FrameIndex,
    pub detections: u32,
    pub strong: u32,
    /// Masked whole; review shows it (1.12.1).
    pub low_confidence: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tracked {
    pub tracks: Vec<TrackSummary>,
    /// Unresolved gaps, one for each span of touching holes of a kind.
    pub gaps: Vec<CoverageEvidence>,
}

fn checkpoint(cancel: &CancelToken) -> Result<(), TrackingError> {
    if cancel.is_cancelled() {
        Err(TrackingError::Cancelled)
    } else {
        Ok(())
    }
}

fn damaged(why: &'static str) -> TrackingError {
    TrackingError::Record(RecordError::Damaged(why))
}

/// The frame's detections of each kind, in the order of `kinds`.
fn split(entries: &[Detected], kinds: &[KindTracking], out: &mut [Vec<Observation>]) {
    for o in out.iter_mut() {
        o.clear();
    }
    for e in entries {
        if let Some(i) = kinds.iter().position(|k| k.kind == e.model.kind()) {
            out[i].push(e.observation);
        }
    }
}

/// Tracks the detections of `record` into a new region record at `regions`, using `scratch` for
/// the backward pass and removing it after.
pub fn track(
    record: &Record<Detected>,
    scratch: &Path,
    regions: &Path,
    kinds: &[KindTracking],
    cancel: &CancelToken,
) -> Result<Tracked, TrackingError> {
    let result = track_with(record, scratch, regions, kinds, cancel);
    match std::fs::remove_file(scratch) {
        Err(e) if e.kind() != io::ErrorKind::NotFound && result.is_ok() => Err(e.into()),
        _ => result,
    }
}

fn track_with(
    record: &Record<Detected>,
    scratch: &Path,
    regions: &Path,
    kinds: &[KindTracking],
    cancel: &CancelToken,
) -> Result<Tracked, TrackingError> {
    assert!(!kinds.is_empty(), "a kind to track");
    let n = record.frames();
    let cuts = SceneCuts::new(record.cuts().iter().copied());
    track_backward(record, scratch, kinds, &cuts, cancel)?;
    let backward = Record::<Assigned>::open(scratch)?;

    let mut mergers: Vec<Merger> = kinds.iter().map(|_| Merger::new(MERGE_IOU)).collect();
    let next = forward(
        record,
        &backward,
        kinds,
        &cuts,
        cancel,
        |_, obs, fwd, bwd| {
            for (i, m) in mergers.iter_mut().enumerate() {
                m.frame(&obs[i], &fwd[i], &bwd[i]);
            }
        },
    )?;
    let joins: Vec<Joins> = mergers
        .into_iter()
        .zip(next)
        .map(|(m, next)| m.finish(next))
        .collect();
    let mut offsets = Vec::with_capacity(kinds.len());
    let mut offset = 0;
    for j in &joins {
        offsets.push(offset);
        offset += j.next_id() - 1;
    }
    let boxes = |i: usize,
                 frame,
                 obs: &[Vec<Observation>],
                 fwd: &[Vec<Assignment>],
                 bwd: &[Vec<Assignment>]| {
        joins[i]
            .frame(&obs[i], &fwd[i], &bwd[i])
            .into_iter()
            .map(|(id, o)| {
                let b = TrackedBox {
                    frame,
                    bbox: o.bbox,
                    score: o.score,
                    interpolated: false,
                    roll: o.roll,
                };
                (id + offsets[i], b)
            })
            .collect::<Vec<_>>()
    };

    let mut summaries: Vec<Summaries> = kinds
        .iter()
        .map(|k| Summaries::new(k.post.strong_score))
        .collect();
    forward(
        record,
        &backward,
        kinds,
        &cuts,
        cancel,
        |frame, obs, fwd, bwd| {
            for (i, s) in summaries.iter_mut().enumerate() {
                s.push(&boxes(i, frame, obs, fwd, bwd));
            }
        },
    )?;
    let summaries: Vec<_> = summaries.into_iter().map(Summaries::finish).collect();

    let mut posts: Vec<PostProcessor> = kinds
        .iter()
        .zip(&summaries)
        .map(|(k, s)| PostProcessor::new(k.post, n, cuts.clone(), s.iter().copied()))
        .collect();
    let mut tracks = Vec::new();
    for ((k, s), post) in kinds.iter().zip(&summaries).zip(&posts) {
        for (id, low_confidence) in post.tracks() {
            let summary = s
                .iter()
                .find(|s| s.id == id)
                .expect("kept tracks are summarized");
            tracks.push(TrackSummary {
                id,
                kind: k.kind,
                first: summary.first,
                last: summary.last,
                detections: summary.real,
                strong: summary.strong,
                low_confidence,
            });
        }
    }
    let mut out = RegionWriter::create(regions, kinds)?;
    let mut write_error = None;
    forward(
        record,
        &backward,
        kinds,
        &cuts,
        cancel,
        |frame, obs, fwd, bwd| {
            for (i, post) in posts.iter_mut().enumerate() {
                let done = post.push(frame, &boxes(i, frame, obs, fwd, bwd));
                out.pending[i].extend(done);
            }
            if write_error.is_none()
                && let Err(e) = out.write_ready()
            {
                write_error = Some(e);
            }
        },
    )?;
    if let Some(e) = write_error {
        return Err(e.into());
    }
    let mut gaps = Vec::new();
    for (i, post) in posts.into_iter().enumerate() {
        let (done, report) = post.finish();
        out.pending[i].extend(done);
        gaps.extend(report.uncovered_spans.into_iter().map(|span| {
            CoverageEvidence::unresolved(Problem::UnresolvedGap {
                detection: kinds[i].kind,
                tracks: span.tracks,
                first: span.first,
                last: span.last,
            })
        }));
    }
    out.write_ready()?;
    out.finish(&cuts)?;
    tracks.sort_by_key(|t| t.id);
    Ok(Tracked { tracks, gaps })
}

fn track_backward(
    record: &Record<Detected>,
    scratch: &Path,
    kinds: &[KindTracking],
    cuts: &SceneCuts,
    cancel: &CancelToken,
) -> Result<(), TrackingError> {
    let n = record.frames();
    // Backward step s reads frame n - 1 - s, which starts a scene in reverse when frame n - s
    // starts one forwards.
    let reversed = cuts.reversed(n);
    let mut trackers: Vec<ByteTracker> = kinds
        .iter()
        .map(|k| ByteTracker::new(k.tracker, 1))
        .collect();
    let mut writer = RecordWriter::<Assigned>::create(scratch, Order::Descending)?;
    let mut frames = record.descending()?;
    let (mut entries, mut assigned) = (Vec::new(), Vec::new());
    let mut obs: Vec<Vec<Observation>> = vec![Vec::new(); kinds.len()];
    let mut step = 0;
    while let Some(frame) = frames.next_into(&mut entries)? {
        checkpoint(cancel)?;
        split(&entries, kinds, &mut obs);
        assigned.clear();
        for (i, t) in trackers.iter_mut().enumerate() {
            let update = t.update(step, reversed.is_cut(step), &obs[i]);
            assigned.extend(update.assigned.into_iter().map(|assignment| Assigned {
                kind: kinds[i].kind,
                assignment,
            }));
        }
        writer.frame(frame, &assigned)?;
        step += 1;
    }
    writer.finish(&[])?;
    Ok(())
}

/// Forward tracking of `record` beside the backward pass, `each` given every frame's detections
/// and both passes' assignments by kind. Returns the number after each kind's forward tracks.
fn forward(
    record: &Record<Detected>,
    backward: &Record<Assigned>,
    kinds: &[KindTracking],
    cuts: &SceneCuts,
    cancel: &CancelToken,
    mut each: impl FnMut(FrameIndex, &[Vec<Observation>], &[Vec<Assignment>], &[Vec<Assignment>]),
) -> Result<Vec<TrackId>, TrackingError> {
    if backward.frames() != record.frames() {
        return Err(damaged("the backward pass has other frames"));
    }
    let mut trackers: Vec<ByteTracker> = kinds
        .iter()
        .map(|k| ByteTracker::new(k.tracker, 1))
        .collect();
    let mut frames = record.ascending()?;
    let mut assignments = backward.ascending()?;
    let (mut entries, mut assigned) = (Vec::new(), Vec::new());
    let mut obs: Vec<Vec<Observation>> = vec![Vec::new(); kinds.len()];
    let mut fwd: Vec<Vec<Assignment>> = vec![Vec::new(); kinds.len()];
    let mut bwd: Vec<Vec<Assignment>> = vec![Vec::new(); kinds.len()];
    while let Some(frame) = frames.next_into(&mut entries)? {
        checkpoint(cancel)?;
        if assignments.next_into(&mut assigned)? != Some(frame) {
            return Err(damaged("the backward pass has other frames"));
        }
        split(&entries, kinds, &mut obs);
        for b in bwd.iter_mut() {
            b.clear();
        }
        for a in &assigned {
            let i = kinds
                .iter()
                .position(|k| k.kind == a.kind)
                .ok_or_else(|| damaged("an assignment of a kind not tracked"))?;
            if a.assignment.detection >= obs[i].len() {
                return Err(damaged("an assignment of no detection"));
            }
            bwd[i].push(a.assignment);
        }
        for (i, t) in trackers.iter_mut().enumerate() {
            fwd[i] = t.update(frame, cuts.is_cut(frame), &obs[i]).assigned;
        }
        each(frame, &obs, &fwd, &bwd);
    }
    Ok(trackers.iter().map(ByteTracker::next_id).collect())
}

/// The post-processed frames of each kind, which come out at different times, written together
/// frame by frame.
struct RegionWriter<'a> {
    writer: RecordWriter<TrackRegion>,
    kinds: &'a [KindTracking],
    pending: Vec<VecDeque<Frame>>,
    entries: Vec<TrackRegion>,
}

impl<'a> RegionWriter<'a> {
    fn create(path: &Path, kinds: &'a [KindTracking]) -> io::Result<Self> {
        Ok(Self {
            writer: RecordWriter::create(path, Order::Ascending)?,
            kinds,
            pending: kinds.iter().map(|_| VecDeque::new()).collect(),
            entries: Vec::new(),
        })
    }

    /// Writes the frames every kind has given.
    fn write_ready(&mut self) -> io::Result<()> {
        while !self.pending.is_empty() && self.pending.iter().all(|p| !p.is_empty()) {
            let mut frame = None;
            self.entries.clear();
            for (i, p) in self.pending.iter_mut().enumerate() {
                let f = p.pop_front().expect("checked");
                assert!(
                    frame.is_none_or(|n| n == f.frame),
                    "kinds give frames in order"
                );
                frame = Some(f.frame);
                self.entries
                    .extend(f.boxes.into_iter().map(|(track, b)| TrackRegion {
                        kind: self.kinds[i].kind,
                        track,
                        bbox: b.bbox,
                        score: b.score,
                        roll: b.roll,
                        interpolated: b.interpolated,
                    }));
            }
            self.writer.frame(frame.expect("a kind"), &self.entries)?;
        }
        Ok(())
    }

    fn finish(self, cuts: &SceneCuts) -> io::Result<()> {
        assert!(
            self.pending.iter().all(VecDeque::is_empty),
            "every frame written"
        );
        self.writer.finish(cuts.frames())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use cloakframe_core::detection::ModelId;
    use cloakframe_core::geometry::Rect;
    use cloakframe_core::tracking::Track;
    use cloakframe_core::tracking::merge::build_bidirectional_tracks;
    use cloakframe_core::tracking::post::{UncoveredSpan, post_process_tracks};
    use std::collections::BTreeMap;

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn unit(&mut self) -> f32 {
            (self.next() % 10_000) as f32 / 10_000.0
        }
    }

    /// Faces and plates moving about, missed now and then, appearing and leaving, and cuts.
    fn video(rng: &mut Rng) -> (Vec<Vec<Detected>>, Vec<FrameIndex>) {
        let n = 40 + rng.next() % 160;
        let subjects: Vec<(ModelId, f32, f32, f32, f32, u64, u64)> = (0..1 + rng.next() % 6)
            .map(|i| {
                let model = if i % 3 == 2 {
                    ModelId::PlateYolov9T
                } else {
                    ModelId::Yolo5FaceN
                };
                let (from, to) = (rng.next() % n, rng.next() % n);
                (
                    model,
                    rng.unit() * 600.0,
                    rng.unit() * 300.0,
                    12.0 + rng.unit() * 60.0,
                    (rng.unit() - 0.5) * 8.0,
                    from.min(to),
                    from.max(to),
                )
            })
            .collect();
        let mut frames = Vec::new();
        for k in 0..n {
            let mut frame = Vec::new();
            for &(model, x, y, side, speed, from, to) in &subjects {
                if !(from..=to).contains(&k) || rng.unit() < 0.25 {
                    continue;
                }
                let jitter = (rng.unit() - 0.5) * side * 0.2;
                let x = x + speed * (k - from) as f32 + jitter;
                let roll = (rng.unit() - 0.5) * 0.6;
                frame.push(Detected {
                    model,
                    observation: Observation {
                        bbox: Rect::new(x, y + jitter, side, side * 0.9).unwrap(),
                        score: 0.1 + rng.unit() * 0.9,
                        roll: (model == ModelId::Yolo5FaceN).then_some(roll),
                    },
                });
            }
            frames.push(frame);
        }
        let mut cuts: Vec<FrameIndex> = (0..rng.next() % 3)
            .map(|_| 1 + rng.next() % (n - 1))
            .collect();
        cuts.sort_unstable();
        cuts.dedup();
        (frames, cuts)
    }

    fn kinds(rate: Rational) -> [KindTracking; 2] {
        [
            KindTracking::new(DetectionKind::Face, 0.4, rate, &TrackingWindows::default()),
            KindTracking::new(DetectionKind::Plate, 0.4, rate, &TrackingWindows::default()),
        ]
    }

    /// What the whole-track reference gives for one kind, numbered from `offset + 1`.
    fn reference(
        frames: &[Vec<Detected>],
        cuts: &SceneCuts,
        k: &KindTracking,
        offset: TrackId,
    ) -> (Vec<Track>, Vec<UncoveredSpan>) {
        let observed: Vec<Vec<Observation>> = frames
            .iter()
            .map(|f| {
                f.iter()
                    .filter(|d| d.model.kind() == k.kind)
                    .map(|d| d.observation)
                    .collect()
            })
            .collect();
        let mut tracks = build_bidirectional_tracks(&observed, &k.tracker, MERGE_IOU, cuts);
        let report = post_process_tracks(&mut tracks, &k.post, frames.len() as u64, cuts);
        for t in &mut tracks {
            t.id += offset;
        }
        let mut spans = report.uncovered_spans;
        for s in &mut spans {
            for t in &mut s.tracks {
                *t += offset;
            }
        }
        (tracks, spans)
    }

    #[test]
    fn tracking_from_the_record_gives_what_whole_tracks_give() {
        let dir = TestDir::new();
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        for case in 0..300 {
            let (frames, cuts) = video(&mut rng);
            // Short windows at a low rate, so holes and bridges across cuts occur. The default
            // windows leave no gap (docs/M5_ANALYSIS.md); half the cases fill shorter holes than
            // the tracker bridges, as 1.12.1 did, to have some.
            let rate = Rational::new([30, 8, 5][case % 3], 1).unwrap();
            let mut kinds = kinds(rate);
            if case % 2 == 1 {
                for k in &mut kinds {
                    k.post.max_interpolation_gap = (k.tracker.max_frames_lost * 2 / 3).max(1);
                }
            }
            let path = |name: &str| dir.path().join(format!("{case}-{name}"));
            let mut writer = RecordWriter::create(&path("detections"), Order::Ascending).unwrap();
            for (k, f) in frames.iter().enumerate() {
                writer.frame(k as FrameIndex, f).unwrap();
            }
            writer.finish(&cuts).unwrap();
            let record = Record::open(&path("detections")).unwrap();
            let tracked = track(
                &record,
                &path("backward"),
                &path("regions"),
                &kinds,
                &CancelToken::new(),
            )
            .unwrap();
            assert!(!path("backward").exists());

            let scene_cuts = SceneCuts::new(cuts.iter().copied());
            let (faces, face_spans) = reference(&frames, &scene_cuts, &kinds[0], 0);
            let face_ids = tracked
                .tracks
                .iter()
                .filter(|t| t.kind == DetectionKind::Face);
            let offset = face_ids.map(|t| t.id).max().unwrap_or(0);
            let (plates, plate_spans) = reference(&frames, &scene_cuts, &kinds[1], offset);

            let mut want: BTreeMap<FrameIndex, Vec<TrackRegion>> = BTreeMap::new();
            for (kind, tracks) in [
                (DetectionKind::Face, &faces),
                (DetectionKind::Plate, &plates),
            ] {
                for t in tracks.iter() {
                    for b in &t.boxes {
                        want.entry(b.frame).or_default().push(TrackRegion {
                            kind,
                            track: t.id,
                            bbox: b.bbox,
                            score: b.score,
                            roll: b.roll,
                            interpolated: b.interpolated,
                        });
                    }
                }
            }
            let regions = Record::<TrackRegion>::open(&path("regions")).unwrap();
            assert_eq!(regions.frames(), frames.len() as u64);
            assert_eq!(regions.cuts(), &cuts[..]);
            let mut got = regions.ascending().unwrap();
            let mut entries = Vec::new();
            while let Some(k) = got.next_into(&mut entries).unwrap() {
                let mut want = want.remove(&k).unwrap_or_default();
                want.sort_by_key(|r| r.track);
                assert_eq!(entries, want, "case {case} frame {k}");
            }

            let want_tracks: Vec<_> = [
                (DetectionKind::Face, &faces),
                (DetectionKind::Plate, &plates),
            ]
            .into_iter()
            .flat_map(|(kind, tracks)| tracks.iter().map(move |t| (t.id, kind, t.low_confidence)))
            .collect();
            let got_tracks: Vec<_> = tracked
                .tracks
                .iter()
                .map(|t| (t.id, t.kind, t.low_confidence))
                .collect();
            assert_eq!(got_tracks, want_tracks, "case {case}");
            let want_gaps: Vec<CoverageEvidence> = [
                (DetectionKind::Face, face_spans),
                (DetectionKind::Plate, plate_spans),
            ]
            .into_iter()
            .flat_map(|(detection, spans)| {
                spans.into_iter().map(move |s| {
                    CoverageEvidence::unresolved(Problem::UnresolvedGap {
                        detection,
                        tracks: s.tracks,
                        first: s.first,
                        last: s.last,
                    })
                })
            })
            .collect();
            assert_eq!(tracked.gaps, want_gaps, "case {case}");
        }
    }

    #[test]
    fn tracking_stops_when_cancelled() {
        let dir = TestDir::new();
        let mut writer =
            RecordWriter::<Detected>::create(&dir.path().join("d"), Order::Ascending).unwrap();
        writer.frame(0, &[]).unwrap();
        writer.finish(&[]).unwrap();
        let cancel = CancelToken::new();
        cancel.cancel();
        let result = track(
            &Record::<Detected>::open(&dir.path().join("d")).unwrap(),
            &dir.path().join("b"),
            &dir.path().join("r"),
            &kinds(Rational::new(30, 1).unwrap()),
            &cancel,
        );
        assert!(matches!(result, Err(TrackingError::Cancelled)));
        assert!(!dir.path().join("b").exists());
    }
}
