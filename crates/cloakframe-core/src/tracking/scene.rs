//! Hard scene cuts, found from tiny greyscale thumbnails of consecutive frames (1.12.1
//! `SceneCutDetector`). A cut ends every track: the subject before it is not the one after.

use std::collections::VecDeque;

use crate::detection::FrameIndex;

pub const THUMBNAIL_WIDTH: usize = 48;
pub const THUMBNAIL_HEIGHT: usize = 27;
const THUMBNAIL_PIXELS: usize = THUMBNAIL_WIDTH * THUMBNAIL_HEIGHT;

const MIN_CUT_DIFF: f32 = 22.0;
const MEDIAN_RATIO: f32 = 2.5;
const RECENT_WINDOW: usize = 8;
/// A cut is committed once this many further frames have not gone back to the content before
/// it, which is how a flash is told from a cut.
const CONFIRM_FRAMES: u32 = 2;

/// The frames that start a new scene, ascending. Frame 0 starts the video and is never a cut.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SceneCuts(Vec<FrameIndex>);

impl SceneCuts {
    pub fn new(frames: impl IntoIterator<Item = FrameIndex>) -> Self {
        let mut frames: Vec<FrameIndex> = frames.into_iter().filter(|&f| f > 0).collect();
        frames.sort_unstable();
        frames.dedup();
        Self(frames)
    }

    pub fn frames(&self) -> &[FrameIndex] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn is_cut(&self, frame: FrameIndex) -> bool {
        self.0.binary_search(&frame).is_ok()
    }

    /// Whether a cut lies after the earlier of the two frames and at or before the later, so the
    /// two are in different scenes.
    pub fn spans_cut(&self, a: FrameIndex, b: FrameIndex) -> bool {
        let (low, high) = (a.min(b), a.max(b));
        self.first_cut_after(low).is_some_and(|c| c <= high)
    }

    pub fn first_cut_after(&self, frame: FrameIndex) -> Option<FrameIndex> {
        let i = self.0.partition_point(|&c| c <= frame);
        self.0.get(i).copied()
    }

    /// The same boundaries in a video of `frame_count` frames read backwards.
    pub fn reversed(&self, frame_count: FrameIndex) -> Self {
        Self::new(
            self.0
                .iter()
                .filter(|&&f| f < frame_count)
                .map(|&f| frame_count - f),
        )
    }
}

/// A 48×27 greyscale picture of a frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail(Box<[u8; THUMBNAIL_PIXELS]>);

impl Thumbnail {
    /// From 8-bit RGB pixels, rows packed. The grey value is OpenCV's fixed-point BT.601 luma, as
    /// 1.12.1's `cvtColor` gave on its BGR frames.
    pub fn from_rgb(width: usize, height: usize, rgb: &[u8]) -> Self {
        assert_eq!(
            rgb.len(),
            width * height * 3,
            "RGB buffer of the stated size"
        );
        let grey: Vec<u8> = rgb
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| {
                let y = u32::from(p[0]) * 4899 + u32::from(p[1]) * 9617 + u32::from(p[2]) * 1868;
                ((y + (1 << 13)) >> 14) as u8
            })
            .collect();
        Self::from_grey(width, height, &grey)
    }

    /// Area-averaged as OpenCV's `INTER_AREA` does: each thumbnail pixel is the mean of the
    /// source area it covers, partial pixels weighted by their covered share.
    pub fn from_grey(width: usize, height: usize, grey: &[u8]) -> Self {
        assert!(width > 0 && height > 0, "a frame with pixels");
        assert_eq!(grey.len(), width * height, "grey buffer of the stated size");
        let xs = area_weights(width, THUMBNAIL_WIDTH);
        let ys = area_weights(height, THUMBNAIL_HEIGHT);
        let mut out = Box::new([0u8; THUMBNAIL_PIXELS]);
        for (ty, row_weights) in ys.iter().enumerate() {
            for (tx, column_weights) in xs.iter().enumerate() {
                let mut sum = 0.0f64;
                let mut weight = 0.0f64;
                for &(sy, wy) in row_weights {
                    for &(sx, wx) in column_weights {
                        sum += f64::from(grey[sy * width + sx]) * wy * wx;
                        weight += wy * wx;
                    }
                }
                out[ty * THUMBNAIL_WIDTH + tx] = (sum / weight).round_ties_even() as u8;
            }
        }
        Self(out)
    }

    fn mean_abs_diff(&self, other: &Thumbnail) -> f32 {
        let total: u32 = self
            .0
            .iter()
            .zip(other.0.iter())
            .map(|(&a, &b)| u32::from(a.abs_diff(b)))
            .sum();
        (f64::from(total) / THUMBNAIL_PIXELS as f64) as f32
    }
}

/// For each of `to` output cells, the source indices it covers and the share of each.
fn area_weights(from: usize, to: usize) -> Vec<Vec<(usize, f64)>> {
    let scale = from as f64 / to as f64;
    (0..to)
        .map(|i| {
            let (start, end) = (i as f64 * scale, (i + 1) as f64 * scale);
            let first = start.floor() as usize;
            let last = (end.ceil() as usize).min(from);
            (first..last)
                .filter_map(|s| {
                    let w = (end.min((s + 1) as f64) - start.max(s as f64)).max(0.0);
                    (w > 0.0).then_some((s, w))
                })
                .collect()
        })
        .collect()
}

struct Candidate {
    frame: FrameIndex,
    diff: f32,
    /// The last frame before the candidate: going back to it means a flash, not a cut.
    before: Thumbnail,
    frames_since: u32,
}

impl Candidate {
    fn went_back(&self, current: &Thumbnail) -> bool {
        current.mean_abs_diff(&self.before) < MIN_CUT_DIFF.min(self.diff * 0.5)
    }
}

/// Fed every frame's thumbnail in order. A frame that differs from the one before by at least
/// 22 grey levels and 2.5 times the recent median is a candidate cut.
#[derive(Default)]
pub struct SceneCutDetector {
    previous: Option<Thumbnail>,
    next_frame: FrameIndex,
    recent: VecDeque<f32>,
    /// Candidates still waiting, oldest first; each came while the one before it waited.
    waiting: Vec<Candidate>,
    cuts: Vec<FrameIndex>,
}

impl SceneCutDetector {
    pub fn push(&mut self, thumbnail: Thumbnail) {
        let frame = self.next_frame;
        self.next_frame += 1;
        let Some(previous) = self.previous.replace(thumbnail) else {
            return;
        };
        let current = self.previous.as_ref().expect("just stored");
        let diff = current.mean_abs_diff(&previous);
        let spike = self.is_spike(diff);
        if !self.waiting.is_empty() {
            // Going back to the content before a waiting candidate makes it and every later one a
            // flash, even one that faded over several frames.
            if let Some(back) = self.waiting.iter().position(|c| c.went_back(current)) {
                self.waiting.truncate(back);
                return;
            }
            // A new spike while the last one waits means the short shot between them was real:
            // both are cuts (P-6). 1.12.1 looked again only after the wait and missed the second.
            if spike {
                self.waiting.push(Candidate {
                    frame,
                    diff,
                    before: previous,
                    frames_since: 0,
                });
                return;
            }
            let last = self.waiting.last_mut().expect("not empty");
            last.frames_since += 1;
            if last.frames_since < CONFIRM_FRAMES {
                return;
            }
            self.cuts.extend(self.waiting.drain(..).map(|c| c.frame));
        }
        if spike {
            self.waiting.push(Candidate {
                frame,
                diff,
                before: previous,
                frames_since: 0,
            });
        } else {
            if self.recent.len() == RECENT_WINDOW {
                self.recent.pop_front();
            }
            self.recent.push_back(diff);
        }
    }

    fn is_spike(&self, diff: f32) -> bool {
        diff >= MIN_CUT_DIFF
            && (self.recent.is_empty() || diff >= self.recent_median() * MEDIAN_RATIO)
    }

    /// The upper median, as 1.12.1's `nth_element` at `size / 2`.
    fn recent_median(&self) -> f32 {
        let mut sorted: Vec<f32> = self.recent.iter().copied().collect();
        sorted.sort_by(f32::total_cmp);
        sorted[sorted.len() / 2]
    }

    /// The cuts, including candidates the video ended on.
    pub fn finish(mut self) -> SceneCuts {
        self.cuts.extend(self.waiting.drain(..).map(|c| c.frame));
        SceneCuts::new(self.cuts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(value: u8) -> Thumbnail {
        Thumbnail::from_grey(480, 270, &vec![value; 480 * 270])
    }

    fn gradient(horizontal: bool) -> Thumbnail {
        let (w, h) = (480usize, 270usize);
        let pixels: Vec<u8> = (0..h)
            .flat_map(|y| {
                (0..w).map(move |x| {
                    if horizontal {
                        (x * 255 / (w - 1)) as u8
                    } else {
                        (y * 255 / (h - 1)) as u8
                    }
                })
            })
            .collect();
        Thumbnail::from_grey(w, h, &pixels)
    }

    fn cuts(scenes: &[(Thumbnail, usize)]) -> Vec<FrameIndex> {
        let mut detector = SceneCutDetector::default();
        for (thumbnail, repeat) in scenes {
            for _ in 0..*repeat {
                detector.push(thumbnail.clone());
            }
        }
        detector.finish().frames().to_vec()
    }

    /// tests/fixtures/legacy/tracking_tests/testSceneCutsContainer.json (normative).
    #[test]
    fn scene_cuts_container() {
        let cuts = SceneCuts::new([15, 5, 5, 0]);
        assert_eq!(cuts.frames(), [5, 15]);
        assert!(cuts.is_cut(5) && !cuts.is_cut(6));
        assert!(cuts.spans_cut(4, 5));
        assert!(cuts.spans_cut(0, 19));
        assert!(cuts.spans_cut(14, 16));
        assert!(!cuts.spans_cut(5, 14));
        assert!(!cuts.spans_cut(15, 19));
        assert!(!SceneCuts::default().spans_cut(0, 1000));
        assert_eq!(cuts.reversed(20).frames(), [5, 15]);
        assert_eq!(SceneCuts::new([7]).reversed(20).frames(), [13]);
        assert_eq!(cuts.first_cut_after(5), Some(15));
        assert_eq!(cuts.first_cut_after(15), None);
    }

    /// The detector tests of tests/fixtures/legacy/tracking_tests (normative), and the P-6 b
    /// cases of tests/fixtures/defects/P-6/expected.json.
    #[test]
    fn hard_cuts_short_shots_and_flashes() {
        let (a, b) = (gradient(true), gradient(false));
        assert_eq!(cuts(&[(a.clone(), 10), (b.clone(), 5)]), [10]);
        assert_eq!(
            cuts(&[(a.clone(), 10), (solid(255), 1), (a.clone(), 10)]),
            []
        );
        assert_eq!(cuts(&[(a.clone(), 30)]), []);
        assert_eq!(cuts(&[(a.clone(), 5), (b, 1)]), [5]);
        for middle in [2, 3, 4] {
            assert_eq!(
                cuts(&[(solid(0), 10), (solid(110), middle), (solid(220), 10)]),
                [10, 10 + middle as FrameIndex]
            );
        }
        assert_eq!(
            cuts(&[(solid(0), 10), (solid(110), 2), (solid(220), 1)]),
            [10, 12]
        );
        // P-6 b: a shot shorter than the wait is a cut at both ends (1.12.1 gave [10] and
        // [10, 12]).
        assert_eq!(
            cuts(&[(solid(0), 10), (solid(255), 1), (solid(110), 10)]),
            [10, 11]
        );
        assert_eq!(
            cuts(&[
                (solid(0), 10),
                (solid(255), 1),
                (solid(110), 1),
                (solid(200), 10)
            ]),
            [10, 11, 12]
        );
        assert_eq!(
            cuts(&[(solid(0), 10), (solid(255), 2), (solid(110), 10)]),
            [10, 12]
        );
        assert_eq!(cuts(&[(a.clone(), 10), (solid(255), 1), (a, 10)]), []);
    }

    /// A flash that fades over several frames is a new spike on each of them, and then the
    /// video goes back to the scene before the first: no cut at all.
    #[test]
    fn a_flash_over_several_frames_that_goes_back_is_no_cut() {
        let a = gradient(true);
        let fade = |steps: &[u8], back: Thumbnail| {
            let mut scenes = vec![(solid(100), 10)];
            scenes.extend(steps.iter().map(|&v| (solid(v), 1)));
            scenes.push((back, 10));
            scenes
        };
        assert_eq!(cuts(&fade(&[255, 180], solid(100))), []);
        assert_eq!(cuts(&fade(&[255, 200, 150], solid(100))), []);
        assert_eq!(cuts(&fade(&[255, 255], solid(100))), []);
        let mut scenes = vec![(a.clone(), 10), (solid(255), 1), (solid(180), 1)];
        scenes.push((a.clone(), 10));
        assert_eq!(cuts(&scenes), []);
        // Back to the first short shot after a flash in it: that shot's cuts stand.
        assert_eq!(
            cuts(&[
                (solid(0), 10),
                (solid(255), 1),
                (solid(110), 1),
                (solid(255), 10)
            ]),
            [10]
        );
    }

    #[test]
    fn thumbnails_average_the_area_each_pixel_covers() {
        // 480 → 48 is ten source columns per thumbnail column.
        let mut pixels = vec![0u8; 480 * 270];
        for row in pixels.as_chunks_mut::<480>().0 {
            row[..5].fill(200);
        }
        let t = Thumbnail::from_grey(480, 270, &pixels);
        assert_eq!((t.0[0], t.0[1]), (100, 0));
        // 50 → 48 splits source pixels between neighbours: the first cell is all of pixel 0 and
        // 1/24 of pixel 1, (0 + 5/24) / (25/24) = 0.2; the last 1/24 of pixel 48 and all of 49,
        // (10 + 245) / (25/24) = 244.8.
        let ramp: Vec<u8> = (0..27).flat_map(|_| (0..50u8).map(|x| x * 5)).collect();
        let t = Thumbnail::from_grey(50, 27, &ramp);
        assert_eq!((t.0[0], t.0[47]), (0, 245));
        // OpenCV's luma weights.
        let t = Thumbnail::from_rgb(1, 1, &[255, 0, 0]);
        assert_eq!(t.0[0], 76);
        let t = Thumbnail::from_rgb(1, 1, &[0, 255, 0]);
        assert_eq!(t.0[0], 150);
    }
}
