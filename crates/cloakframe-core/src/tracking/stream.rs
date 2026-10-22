//! The post-processing of [`super::post`] fed the tracks' boxes a frame at a time and giving the
//! processed boxes back in frame order, so a video of any length is processed holding only the
//! tracks near the current frame: each track's last few boxes for interpolating and smoothing,
//! and the ends and starts of tracks around the next cut. Every track's first and last frame
//! must be known up front ([`Summaries`], one pass earlier), as a whole-track step needs them:
//! whether the track is kept, and where it ends.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::detection::FrameIndex;

use super::post::{CoverageReport, UncoveredSpan, fill, is_low_confidence, merge_spans, smoothed};
use super::scene::SceneCuts;
use super::{PostProcessConfig, TrackId, TrackedBox};

/// What post-processing needs to know about a track before its boxes arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackSummary {
    pub id: TrackId,
    pub first: FrameIndex,
    pub last: FrameIndex,
    /// Detected boxes, and those at or above the strong score.
    pub real: u32,
    pub strong: u32,
}

/// Gathers the summaries from the tracks' boxes, frame by frame.
pub struct Summaries {
    strong_score: f32,
    tracks: BTreeMap<TrackId, TrackSummary>,
}

impl Summaries {
    pub fn new(strong_score: f32) -> Self {
        Self {
            strong_score,
            tracks: BTreeMap::new(),
        }
    }

    pub fn push(&mut self, boxes: &[(TrackId, TrackedBox)]) {
        for &(id, b) in boxes {
            let s = self.tracks.entry(id).or_insert(TrackSummary {
                id,
                first: b.frame,
                last: b.frame,
                real: 0,
                strong: 0,
            });
            s.last = b.frame;
            if !b.interpolated {
                s.real += 1;
                if b.score >= self.strong_score {
                    s.strong += 1;
                }
            }
        }
    }

    pub fn finish(self) -> Vec<TrackSummary> {
        self.tracks.into_values().collect()
    }
}

/// The processed boxes of one frame, by track.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub frame: FrameIndex,
    pub boxes: Vec<(TrackId, TrackedBox)>,
}

/// One running track: interpolation, smoothing and the held-over ends.
struct Pipe {
    last: FrameIndex,
    real: u32,
    last_real: Option<TrackedBox>,
    /// Boxes after interpolation, unsmoothed: up to `radius` already given out, for their
    /// neighbours, then the ones waiting for theirs.
    window: VecDeque<TrackedBox>,
    next: usize,
    boxes_so_far: u64,
    started: bool,
}

pub struct PostProcessor {
    config: PostProcessConfig,
    frame_count: FrameIndex,
    cuts: SceneCuts,
    kept: BTreeMap<TrackId, TrackSummary>,
    dropped: u32,
    /// Kept tracks whose first box has not arrived, by first frame.
    upcoming: BTreeSet<(FrameIndex, TrackId)>,
    /// Frames before this one have all been pushed.
    next_frame: FrameIndex,
    pipes: BTreeMap<TrackId, Pipe>,
    /// Ended tracks that may reach across the cut after them: (cut, track, last box).
    ended: Vec<(FrameIndex, TrackId, TrackedBox)>,
    /// First boxes at or after the next undecided cut, by frame and track.
    starts: BTreeMap<(FrameIndex, TrackId), TrackedBox>,
    /// Boxes carried back from a cut to a track's start: first frame, box, start.
    bridged: BTreeMap<TrackId, (FrameIndex, TrackedBox, FrameIndex)>,
    /// Index into the cuts of the first one not yet decided.
    next_cut: usize,
    out: BTreeMap<FrameIndex, Vec<(TrackId, TrackedBox)>>,
    emitted: FrameIndex,
    spans: Vec<UncoveredSpan>,
}

impl PostProcessor {
    pub fn new(
        config: PostProcessConfig,
        frame_count: FrameIndex,
        cuts: SceneCuts,
        summaries: impl IntoIterator<Item = TrackSummary>,
    ) -> Self {
        let mut kept = BTreeMap::new();
        let mut dropped = 0;
        for s in summaries {
            if config.retain_without_strong || s.strong > 0 {
                kept.insert(s.id, s);
            } else {
                dropped += 1;
            }
        }
        let upcoming = kept.values().map(|s| (s.first, s.id)).collect();
        Self {
            config,
            frame_count,
            cuts,
            kept,
            dropped,
            upcoming,
            next_frame: 0,
            pipes: BTreeMap::new(),
            ended: Vec::new(),
            starts: BTreeMap::new(),
            bridged: BTreeMap::new(),
            next_cut: 0,
            out: BTreeMap::new(),
            emitted: 0,
            spans: Vec::new(),
        }
    }

    /// The tracks that are kept and whether each is low confidence.
    pub fn tracks(&self) -> impl Iterator<Item = (TrackId, bool)> + '_ {
        self.kept
            .values()
            .map(|s| (s.id, is_low_confidence(s.strong, s.real, &self.config)))
    }

    /// The tracks' boxes in `frame`, which comes after every frame pushed before. Boxes of tracks
    /// that are not kept are ignored. Gives back every frame that is now final.
    pub fn push(&mut self, frame: FrameIndex, boxes: &[(TrackId, TrackedBox)]) -> Vec<Frame> {
        assert!(frame >= self.next_frame, "frames in order");
        for &(id, b) in boxes {
            let Some(summary) = self.kept.get(&id) else {
                continue;
            };
            assert_eq!(b.frame, frame, "boxes of the frame pushed");
            let pipe = self.pipes.entry(id).or_insert_with(|| Pipe {
                last: summary.last,
                real: summary.real,
                last_real: None,
                window: VecDeque::new(),
                next: 0,
                boxes_so_far: 0,
                started: false,
            });
            self.upcoming.remove(&(summary.first, id));
            if let Some(a) = pipe.last_real {
                let gap = b.frame - a.frame - 1;
                let max = self.config.max_interpolation_gap;
                if gap >= 1 && max >= 1 && !self.cuts.spans_cut(a.frame, b.frame) {
                    if gap > max {
                        self.spans.push(UncoveredSpan {
                            first: a.frame + 1,
                            last: b.frame - 1,
                            tracks: vec![id],
                        });
                    } else {
                        for f in fill(&a, &b, gap) {
                            pipe.window.push_back(f);
                            pipe.boxes_so_far += 1;
                        }
                    }
                }
            }
            pipe.window.push_back(b);
            pipe.boxes_so_far += 1;
            pipe.last_real = Some(b);
        }
        self.next_frame = frame + 1;
        self.advance()
    }

    /// Everything still held. Every kept track's boxes must have been pushed.
    pub fn finish(mut self) -> (Vec<Frame>, CoverageReport) {
        self.next_frame = self.next_frame.max(self.frame_count);
        assert!(
            self.upcoming.is_empty()
                && self
                    .pipes
                    .values()
                    .all(|p| p.last_real.is_some_and(|b| b.frame == p.last)),
            "every kept track's boxes pushed"
        );
        let frames = self.advance();
        assert!(self.pipes.is_empty() && self.out.is_empty());
        let uncovered_spans = merge_spans(std::mem::take(&mut self.spans));
        let report = CoverageReport {
            uncovered_frames: uncovered_spans.iter().map(UncoveredSpan::frame_count).sum(),
            uncovered_spans,
            dropped_tracks: self.dropped,
        };
        (frames, report)
    }

    fn advance(&mut self) -> Vec<Frame> {
        let ids: Vec<TrackId> = self.pipes.keys().copied().collect();
        for id in ids {
            self.smooth(id);
        }
        while self.next_cut < self.cuts.frames().len() && self.cut_decidable() {
            self.decide_cut();
        }
        let ready = self.ready().min(self.frame_count);
        let mut frames = Vec::new();
        while self.emitted < ready {
            let mut boxes = self.out.remove(&self.emitted).unwrap_or_default();
            boxes.sort_by_key(|&(id, _)| id);
            frames.push(Frame {
                frame: self.emitted,
                boxes,
            });
            self.emitted += 1;
        }
        frames
    }

    /// The earliest frame of a box the track may still get from interpolation: the frame after
    /// its last detection while a later one could still be joined to it, otherwise the next
    /// frame to be pushed.
    fn next_possible(&self, pipe: &Pipe) -> FrameIndex {
        match pipe.last_real {
            Some(a) if a.frame == pipe.last => FrameIndex::MAX,
            Some(a) => {
                let max = self.config.max_interpolation_gap;
                let can_fill = max >= 1
                    && self.next_frame - a.frame - 1 <= max
                    && !self.cuts.spans_cut(a.frame, self.next_frame);
                if can_fill {
                    a.frame + 1
                } else {
                    self.next_frame
                }
            }
            None => self.next_frame,
        }
    }

    /// Smooths and gives out every box of the track whose neighbours are all known.
    fn smooth(&mut self, id: TrackId) {
        let radius = self.config.smoothing_radius;
        let ext = self.config.extension_frames;
        loop {
            let pipe = &self.pipes[&id];
            let Some(&current) = pipe.window.get(pipe.next) else {
                return;
            };
            if self.next_possible(pipe) <= current.frame + radius {
                return;
            }
            let pipe = self.pipes.get_mut(&id).expect("running track");
            let enabled = radius >= 1 && (pipe.real >= 3 || pipe.boxes_so_far >= 3);
            let s = if enabled {
                smoothed(pipe.window.make_contiguous(), pipe.next, radius)
            } else {
                current
            };
            pipe.next += 1;
            while pipe.next > radius as usize {
                pipe.window.pop_front();
                pipe.next -= 1;
            }
            let first_out = !pipe.started;
            pipe.started = true;
            let is_last = s.frame == pipe.last;
            let held = |frame| TrackedBox {
                frame,
                interpolated: true,
                ..s
            };
            let mut start = s;
            if first_out && self.frame_count >= 1 {
                for f in s.frame.saturating_sub(ext)..s.frame {
                    if !self.cuts.spans_cut(f, s.frame) {
                        let b = held(f);
                        if start.frame == s.frame {
                            start = b;
                        }
                        self.emit(id, b);
                    }
                }
            }
            self.emit(id, s);
            if first_out {
                self.register_start(id, start);
            }
            if is_last {
                let mut end = s;
                if self.frame_count >= 1 {
                    let last_allowed = (self.frame_count - 1).min(s.frame + ext);
                    for f in s.frame + 1..=last_allowed {
                        if self.cuts.spans_cut(s.frame, f) {
                            break;
                        }
                        end = held(f);
                        self.emit(id, end);
                    }
                }
                self.pipes.remove(&id);
                if let Some(cut) = self.cuts.first_cut_after(end.frame) {
                    self.ended.push((cut, id, end));
                }
                return;
            }
        }
    }

    fn emit(&mut self, id: TrackId, b: TrackedBox) {
        debug_assert!(
            b.frame >= self.emitted,
            "frame {} already given out",
            b.frame
        );
        self.out.entry(b.frame).or_default().push((id, b));
    }

    fn pending_cut(&self) -> Option<FrameIndex> {
        self.cuts.frames().get(self.next_cut).copied()
    }

    fn register_start(&mut self, id: TrackId, start: TrackedBox) {
        if self.pending_cut().is_some_and(|c| start.frame >= c) {
            self.starts.insert((start.frame, id), start);
        }
    }

    /// Whether every track that ends before the next cut has ended, and every track that may
    /// start within the bridging distance after it has given out its first box.
    fn cut_decidable(&self) -> bool {
        let cut = self.pending_cut().expect("a cut to decide");
        let reach = cut + self.config.max_cut_boundary_gap + self.config.extension_frames;
        let all_before_ended = self.pipes.values().all(|p| p.last >= cut)
            && self.upcoming.first().is_none_or(|&(first, _)| first >= cut);
        let all_after_started = self
            .upcoming
            .first()
            .is_none_or(|&(first, _)| first > reach)
            && self
                .pipes
                .iter()
                .all(|(id, p)| p.started || self.kept[id].first > reach);
        all_before_ended && all_after_started
    }

    /// The tracks either side of the cut that meet in the same place (1.12.1
    /// `postProcessTracks`): the hole between them is covered by both boxes when it is no longer
    /// than an interpolated one, each track taking the frames on its side, and reported when it
    /// is longer.
    fn decide_cut(&mut self) {
        let cut = self.pending_cut().expect("a cut to decide");
        let max_gap = self.config.max_cut_boundary_gap;
        let ended: Vec<(TrackId, TrackedBox)> = self
            .ended
            .iter()
            .filter(|&&(c, _, _)| c == cut)
            .map(|&(_, id, end)| (id, end))
            .collect();
        self.ended.retain(|&(c, _, _)| c != cut);
        for (before, end) in ended {
            let lowest = (end.frame + 2).max(cut);
            let candidates = self
                .starts
                .range((lowest, TrackId::MIN)..)
                .take_while(|&(&(s, _), _)| s - end.frame - 1 <= max_gap);
            let Some((&(_, after), &start)) = candidates
                .into_iter()
                .find(|&(_, b)| end.bbox.intersection_area(&b.bbox) > 0.0)
            else {
                continue;
            };
            let gap = start.frame - end.frame - 1;
            if gap <= self.config.max_interpolation_gap {
                let bridge = TrackedBox {
                    frame: 0,
                    bbox: end.bbox.union(&start.bbox),
                    score: end.score.min(start.score),
                    interpolated: true,
                    roll: None,
                };
                for frame in end.frame + 1..cut {
                    self.emit(before, TrackedBox { frame, ..bridge });
                }
                self.bridged
                    .entry(after)
                    .and_modify(|(first, b, _)| {
                        *first = (*first).min(cut);
                        b.bbox = b.bbox.union(&bridge.bbox);
                        b.score = b.score.min(bridge.score);
                    })
                    .or_insert((cut, bridge, start.frame));
            } else {
                self.spans.push(UncoveredSpan {
                    first: end.frame + 1,
                    last: start.frame - 1,
                    tracks: vec![before, after],
                });
            }
        }
        self.next_cut += 1;
        let next = self.pending_cut().unwrap_or(FrameIndex::MAX);
        self.starts.retain(|&(s, _), _| s >= next);
        // A start is final once no cut at or before it is left to bridge into it.
        let ready: Vec<TrackId> = self
            .bridged
            .iter()
            .filter(|&(_, &(_, _, start))| start < next)
            .map(|(&id, _)| id)
            .collect();
        for id in ready {
            let (first, b, start) = self.bridged.remove(&id).expect("listed");
            for frame in first..start {
                self.emit(id, TrackedBox { frame, ..b });
            }
        }
    }

    /// The first frame that may still get a box. Boxes come only from the running tracks, the
    /// tracks still to start (whose first boxes are held back over `extension_frames` earlier
    /// frames), the cut still to decide and starts waiting for their bridge.
    fn ready(&self) -> FrameIndex {
        let ext = self.config.extension_frames;
        let mut ready = FrameIndex::MAX;
        if let Some(&(first, _)) = self.upcoming.first() {
            ready = ready.min(first.saturating_sub(ext));
        }
        for pipe in self.pipes.values() {
            let earliest = match pipe.window.get(pipe.next) {
                Some(b) if pipe.started => b.frame,
                Some(b) => b.frame.saturating_sub(ext),
                None => self.next_possible(pipe),
            };
            ready = ready.min(earliest);
        }
        if let Some(cut) = self.pending_cut() {
            ready = ready.min(cut.saturating_sub(self.config.max_cut_boundary_gap));
        }
        for &(first, _, _) in self.bridged.values() {
            ready = ready.min(first);
        }
        ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;
    use crate::tracking::Track;
    use crate::tracking::post::post_process_tracks;

    impl PostProcessor {
        /// Boxes held anywhere inside.
        fn held(&self) -> usize {
            self.pipes.values().map(|p| p.window.len()).sum::<usize>()
                + self.out.values().map(Vec::len).sum::<usize>()
                + self.ended.len()
                + self.starts.len()
                + self.bridged.len()
        }
    }

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n.max(1)
        }

        fn chance(&mut self, percent: u64) -> bool {
            self.below(100) < percent
        }
    }

    /// Feeds the tracks a frame at a time and collects the output back into tracks.
    fn streamed(
        tracks: &[Track],
        config: &PostProcessConfig,
        frame_count: FrameIndex,
        cuts: &SceneCuts,
    ) -> (Vec<Track>, CoverageReport, usize) {
        let per_frame = |k: FrameIndex| -> Vec<(TrackId, TrackedBox)> {
            tracks
                .iter()
                .filter_map(|t| t.box_at(k).map(|b| (t.id, *b)))
                .collect()
        };
        let mut summaries = Summaries::new(config.strong_score);
        for k in 0..frame_count {
            summaries.push(&per_frame(k));
        }
        let mut p = PostProcessor::new(*config, frame_count, cuts.clone(), summaries.finish());
        let low: BTreeMap<TrackId, bool> = p.tracks().collect();
        let mut frames = Vec::new();
        let mut most_held = 0;
        for k in 0..frame_count {
            frames.extend(p.push(k, &per_frame(k)));
            most_held = most_held.max(p.held());
        }
        let (rest, report) = p.finish();
        frames.extend(rest);
        let order: Vec<FrameIndex> = frames.iter().map(|f| f.frame).collect();
        assert_eq!(
            order,
            (0..frame_count).collect::<Vec<_>>(),
            "every frame once, in order"
        );
        let mut out: BTreeMap<TrackId, Vec<TrackedBox>> = BTreeMap::new();
        for f in frames {
            for (id, b) in f.boxes {
                out.entry(id).or_default().push(b);
            }
        }
        let tracks = low
            .into_iter()
            .map(|(id, low_confidence)| Track {
                id,
                low_confidence,
                boxes: out.remove(&id).unwrap_or_default(),
            })
            .collect();
        (tracks, report, most_held)
    }

    fn random_case(rng: &mut Rng) -> (Vec<Track>, PostProcessConfig, FrameIndex, SceneCuts) {
        let frame_count = 1 + rng.below(70);
        let cuts = SceneCuts::new((0..frame_count).filter(|_| rng.chance(8)));
        let config = PostProcessConfig {
            max_interpolation_gap: rng.below(7),
            smoothing_radius: rng.below(4),
            extension_frames: rng.below(5),
            max_cut_boundary_gap: rng.below(9),
            strong_score: [0.5, 0.95][rng.below(2) as usize],
            min_strong: 1 + rng.below(4) as u32,
            short_track_min_strong: rng.below(3) as u32,
            short_track_strong_ratio: 0.5,
            retain_without_strong: rng.chance(30),
        };
        let mut tracks = Vec::new();
        for id in 1..=rng.below(7) as TrackId {
            let first = rng.below(frame_count);
            let mut boxes = Vec::new();
            let mut frame = first;
            // Tracks near one another and of similar size, so cuts find neighbours to bridge.
            let (mut x, mut y) = (50.0 + rng.below(60) as f32, 50.0 + rng.below(60) as f32);
            let mut size = 20.0 + rng.below(30) as f32;
            while frame < frame_count && boxes.len() < 40 {
                if rng.chance(70) || boxes.is_empty() {
                    boxes.push(TrackedBox {
                        frame,
                        bbox: Rect::new(x, y, size, size).unwrap(),
                        score: [0.2, 0.6, 0.97][rng.below(3) as usize],
                        interpolated: false,
                        roll: rng
                            .chance(40)
                            .then(|| (rng.below(200) as f32 - 100.0) / 100.0),
                    });
                }
                if rng.chance(12) {
                    break;
                }
                frame += 1 + if rng.chance(20) { rng.below(9) } else { 0 };
                x += rng.below(9) as f32 - 4.0;
                y += rng.below(9) as f32 - 4.0;
                if rng.chance(10) {
                    size *= 1.0 + rng.below(150) as f32 / 100.0;
                }
            }
            tracks.push(Track {
                id,
                low_confidence: false,
                boxes,
            });
        }
        (tracks, config, frame_count, cuts)
    }

    #[test]
    fn frames_in_order_give_what_whole_tracks_give() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut bridged = 0;
        for case in 0..3000 {
            let (tracks, config, frame_count, cuts) = random_case(&mut rng);
            let mut reference = tracks.clone();
            let want = post_process_tracks(&mut reference, &config, frame_count, &cuts);
            let (got, report, _) = streamed(&tracks, &config, frame_count, &cuts);
            assert_eq!(report, want, "case {case}: {config:?} {cuts:?} {tracks:?}");
            assert_eq!(
                got, reference,
                "case {case}: {config:?} {cuts:?} {tracks:?}"
            );
            bridged += reference
                .iter()
                .filter(|t| t.boxes.iter().any(|b| b.interpolated && b.roll.is_none()))
                .count();
        }
        assert!(bridged > 100, "the cases reach the bridging");
    }

    /// One subject in every frame of a long video: what is held does not grow with the length.
    #[test]
    fn what_is_held_does_not_grow_with_the_video() {
        let held = |frame_count: FrameIndex| {
            let boxes = (0..frame_count)
                .filter(|k| k % 7 != 3)
                .map(|k| TrackedBox {
                    frame: k,
                    bbox: Rect::new((k % 500) as f32, 10.0, 40.0, 40.0).unwrap(),
                    score: 0.9,
                    interpolated: false,
                    roll: None,
                })
                .collect();
            let tracks = [Track {
                id: 1,
                low_confidence: false,
                boxes,
            }];
            let cuts = SceneCuts::new((1..frame_count / 1000).map(|i| i * 1000));
            streamed(&tracks, &PostProcessConfig::default(), frame_count, &cuts).2
        };
        let (short, long) = (held(2_000), held(60_000));
        assert_eq!(short, long);
        assert!(long < 100, "{long}");
    }
}
