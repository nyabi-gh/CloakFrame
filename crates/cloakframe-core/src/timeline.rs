//! The one description of a video's timeline that analysis, preview and export all use (plan 4.4,
//! P-3). It is built once from ffprobe's report. Passes read the source frames unfiltered, in
//! presentation order, and [`SourceFrames`] decides which of them each frame `n` of the
//! constant-rate output shows, so no FFmpeg filter state takes part in matching frames.

use std::cmp::Ordering;
use std::fmt;

/// A positive fraction such as a frame rate or a sample aspect ratio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rational {
    num: u32,
    den: u32,
}

impl Rational {
    pub fn new(num: u32, den: u32) -> Option<Self> {
        (num > 0 && den > 0).then(|| {
            let g = gcd(u64::from(num), u64::from(den)) as u32;
            Self {
                num: num / g,
                den: den / g,
            }
        })
    }

    pub fn num(self) -> u32 {
        self.num
    }

    pub fn den(self) -> u32 {
        self.den
    }

    pub fn as_f64(self) -> f64 {
        f64::from(self.num) / f64::from(self.den)
    }
}

impl Ord for Rational {
    fn cmp(&self, other: &Self) -> Ordering {
        (u64::from(self.num) * u64::from(other.den))
            .cmp(&(u64::from(other.num) * u64::from(self.den)))
    }
}

impl PartialOrd for Rational {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn gcd128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// An exact point in time, in seconds, reduced to lowest terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seconds {
    num: i128,
    den: u128,
}

impl Seconds {
    pub const ZERO: Self = Self { num: 0, den: 1 };

    pub fn new(num: i128, den: u128) -> Option<Self> {
        if den == 0 {
            return None;
        }
        let g = gcd128(num.unsigned_abs(), den).max(1);
        Some(Self {
            num: num / g as i128,
            den: den / g,
        })
    }

    pub fn num(self) -> i128 {
        self.num
    }

    pub fn den(self) -> u128 {
        self.den
    }

    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// `ticks` of `time_base`.
    pub fn of_ticks(ticks: i64, time_base: Rational) -> Self {
        Self::new(
            i128::from(ticks) * i128::from(time_base.num()),
            u128::from(time_base.den()),
        )
        .expect("a positive denominator")
    }
}

impl Ord for Seconds {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.num * other.den as i128).cmp(&(other.num * self.den as i128))
    }
}

impl PartialOrd for Seconds {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    Hevc,
    Vp8,
    Vp9,
}

/// The coded picture is turned this far clockwise for display. FFmpeg applies it while
/// decoding, so every pass sees display-oriented frames and the output carries no rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    None,
    Quarter,
    Half,
    ThreeQuarters,
}

impl Rotation {
    pub fn swaps_sides(self) -> bool {
        matches!(self, Self::Quarter | Self::ThreeQuarters)
    }
}

/// The colour description shared by decode and encode (D1). A field the source leaves out is
/// assumed from the frame size and marked in [`ColorSpec::assumed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorSpec {
    pub matrix: Matrix,
    pub primaries: Primaries,
    pub transfer: Transfer,
    pub range: Range,
    pub assumed: bool,
}

/// The names are FFmpeg's, as ffprobe reports them and the encoder's colour options take them;
/// the older option names some values also have are read as well.
macro_rules! ffmpeg_names {
    ($(#[$doc:meta])* $name:ident { $($variant:ident = $text:literal $(| $alias:literal)*),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub fn ffmpeg_name(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            pub fn from_ffmpeg_name(name: &str) -> Option<Self> {
                match name {
                    $($text $(| $alias)* => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

ffmpeg_names!(
    /// The matrices the decoder's scaler takes. `gbr` is RGB-coded video, which reaches it as
    /// planar RGB and needs no matrix.
    Matrix {
        Bt709 = "bt709",
        Bt470bg = "bt470bg",
        Smpte170m = "smpte170m",
        Smpte240m = "smpte240m",
        Fcc = "fcc",
        Bt2020nc = "bt2020nc",
        Gbr = "gbr",
    }
);

ffmpeg_names!(Primaries {
    Bt709 = "bt709",
    Bt470m = "bt470m",
    Bt470bg = "bt470bg",
    Smpte170m = "smpte170m",
    Smpte240m = "smpte240m",
    Film = "film",
    Bt2020 = "bt2020",
    Smpte428 = "smpte428",
    Smpte431 = "smpte431",
    Smpte432 = "smpte432",
    Ebu3213 = "ebu3213" | "jedec-p22",
});

ffmpeg_names!(
    /// SDR transfers only; PQ and HLG are refused before a spec is made (until M9).
    Transfer {
        Bt709 = "bt709",
        Bt470m = "bt470m" | "gamma22",
        Bt470bg = "bt470bg" | "gamma28",
        Smpte170m = "smpte170m",
        Smpte240m = "smpte240m",
        Linear = "linear",
        Log100 = "log100",
        Log316 = "log316",
        Iec61966_2_1 = "iec61966-2-1",
        Iec61966_2_4 = "iec61966-2-4",
        Bt1361e = "bt1361e",
        Bt2020_10 = "bt2020-10",
        Bt2020_12 = "bt2020-12",
        Smpte428 = "smpte428",
    }
);

ffmpeg_names!(Range {
    Tv = "tv",
    Pc = "pc",
});

/// The fields the source tagged; the rest are assumed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ColorTags {
    pub matrix: Option<Matrix>,
    pub primaries: Option<Primaries>,
    pub transfer: Option<Transfer>,
    pub range: Option<Range>,
}

/// Untagged frames this wide, or taller than [`SD_MAX_HEIGHT`], are taken as HD (BT.709), the
/// rest as SD (SMPTE 170M), in limited range. Players such as mpv guess the same way, so a wide
/// crop of HD (1280x544) stays HD.
pub const HD_MIN_WIDTH: u32 = 1280;
pub const SD_MAX_HEIGHT: u32 = 576;

impl ColorTags {
    /// The source's tags, with the untagged fields assumed from its coded size.
    pub fn complete(self, width: u32, height: u32) -> ColorSpec {
        let hd = width >= HD_MIN_WIDTH || height > SD_MAX_HEIGHT;
        ColorSpec {
            matrix: self
                .matrix
                .unwrap_or(if hd { Matrix::Bt709 } else { Matrix::Smpte170m }),
            primaries: self.primaries.unwrap_or(if hd {
                Primaries::Bt709
            } else {
                Primaries::Smpte170m
            }),
            transfer: self.transfer.unwrap_or(if hd {
                Transfer::Bt709
            } else {
                Transfer::Smpte170m
            }),
            range: self.range.unwrap_or(Range::Tv),
            assumed: self.matrix.is_none()
                || self.primaries.is_none()
                || self.transfer.is_none()
                || self.range.is_none(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioStream {
    /// The stream's index in the container.
    pub index: u32,
    pub codec: String,
    /// An ISO 639-2 code, the form MP4 stores; `und` and anything else is dropped.
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineSpec {
    /// The video stream's index in the container.
    pub stream: u32,
    pub codec: VideoCodec,
    pub coded_width: u32,
    pub coded_height: u32,
    pub rotation: Rotation,
    pub sample_aspect: Rational,
    /// The constant rate of every pass. ffprobe's report alone gives a provisional one for the
    /// limits; [`SourceFrames::constant_rate`] gives the final one from the frame list.
    pub rate: Rational,
    /// `r_frame_rate`: FFmpeg's guess from the first frames, not a rate the file declares.
    pub nominal_rate: Option<Rational>,
    pub average_rate: Option<Rational>,
    pub variable_rate: bool,
    /// When the first video frame is shown, never before zero.
    pub start: Seconds,
    /// The video stream's time base, in which [`SourceFrames`] holds its timestamps.
    pub time_base: Rational,
    /// The container's start time, which FFmpeg adds to a seek position. For seeking only.
    pub container_start_seconds: f64,
    /// `None` when the report gives none (WebM written as it streams) until the frame list does.
    pub duration_seconds: Option<f64>,
    /// For limits and progress: estimated from the report, then the output's own count once the
    /// frame list is read.
    pub estimated_frames: u64,
    pub color: ColorSpec,
    pub audio: Vec<AudioStream>,
    /// Other video streams (not cover art) that are not processed and not kept.
    pub other_video_streams: u32,
}

impl TimelineSpec {
    /// The frame size every pass sees, after rotation.
    pub fn display_size(&self) -> (u32, u32) {
        if self.rotation.swaps_sides() {
            (self.coded_height, self.coded_width)
        } else {
            (self.coded_width, self.coded_height)
        }
    }

    /// When frame `n` starts on the constant-rate timeline.
    pub fn frame_start(&self, n: u64) -> Seconds {
        self.at(2 * u128::from(n))
    }

    /// The middle of frame `n`'s time on the constant-rate timeline; the frame shows the source
    /// frame on screen at that moment.
    pub fn frame_middle(&self, n: u64) -> Seconds {
        self.at(2 * u128::from(n) + 1)
    }

    /// `start + half_frames / (2 * rate)`.
    fn at(&self, half_frames: u128) -> Seconds {
        let rate_num = u128::from(self.rate.num());
        let rate_den = u128::from(self.rate.den());
        let den = self.start.den() * rate_num * 2;
        let num = self.start.num() * (rate_num * 2) as i128
            + (half_frames * rate_den * self.start.den()) as i128;
        Seconds::new(num, den).expect("a nonzero denominator")
    }
}

/// Rates an output may take besides the source's own, lowest first. The last is also the input
/// limit.
const STANDARD_RATES: [(u32, u32); 14] = [
    (24000, 1001),
    (24, 1),
    (25, 1),
    (30000, 1001),
    (30, 1),
    (48, 1),
    (50, 1),
    (60000, 1001),
    (60, 1),
    (90, 1),
    (100, 1),
    (120, 1),
    (144, 1),
    (240, 1),
];

/// The integer rate next to an NTSC one (30 for 29.97).
fn integer_twin(rate: Rational) -> Option<Rational> {
    (rate.den() == 1001).then(|| Rational::new(rate.num() / 1000, 1).expect("a positive rate"))
}

/// The processed stream's frames in presentation order, as the packet list gives them without
/// decoding; frame `k` of an unfiltered pass is the `k`-th.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFrames {
    time_base: Rational,
    /// Strictly increasing.
    pts: Vec<i64>,
    /// When the last frame stops showing.
    end: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFramesError {
    Empty,
    /// Two frames share a timestamp, so their order cannot be told.
    SameTime,
}

impl fmt::Display for SourceFramesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "the video stream has no frames",
            Self::SameTime => "two video frames have the same timestamp",
        })
    }
}

impl std::error::Error for SourceFramesError {}

impl SourceFrames {
    /// `pts` in any order (packets come in decoding order); `last_duration` is the duration
    /// the container gives the last frame in presentation order, if any.
    pub fn new(
        time_base: Rational,
        mut pts: Vec<i64>,
        last_duration: Option<i64>,
    ) -> Result<Self, SourceFramesError> {
        pts.sort_unstable();
        if pts.windows(2).any(|w| w[0] == w[1]) {
            return Err(SourceFramesError::SameTime);
        }
        let (Some(&last), previous) = (pts.last(), pts.iter().rev().nth(1)) else {
            return Err(SourceFramesError::Empty);
        };
        let shown_for = last_duration
            .filter(|&d| d > 0)
            .or_else(|| previous.map(|&p| last - p))
            .unwrap_or(1);
        Ok(Self {
            time_base,
            pts,
            end: last.saturating_add(shown_for),
        })
    }

    pub fn len(&self) -> usize {
        self.pts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pts.is_empty()
    }

    pub fn pts(&self, k: usize) -> i64 {
        self.pts[k]
    }

    pub fn time(&self, k: usize) -> Seconds {
        Seconds::of_ticks(self.pts[k], self.time_base)
    }

    /// When the last frame stops showing.
    pub fn end(&self) -> Seconds {
        Seconds::of_ticks(self.end, self.time_base)
    }

    /// How many constant-rate frames the output has: those whose middle falls before the last
    /// source frame stops showing.
    pub fn output_frames(&self, spec: &TimelineSpec) -> u64 {
        let end = Seconds::of_ticks(self.end, self.time_base);
        if spec.frame_middle(0) >= end {
            return 0;
        }
        // frame_middle(n) < end  <=>  n < (end - start) * rate - 1/2
        let rate = spec.rate;
        let span_num = end.num() * spec.start.den() as i128 - spec.start.num() * end.den() as i128;
        let span_den = end.den() * spec.start.den();
        let a = 2 * span_num * i128::from(rate.num());
        let b = (span_den * u128::from(rate.den())) as i128;
        ((a - b - 1).div_euclid(2 * b) + 1) as u64
    }

    /// The source frame output frame `n` shows: the last one on screen at its middle, or the
    /// first one when none has started yet.
    pub fn shown_at(&self, spec: &TimelineSpec, n: u64) -> usize {
        let middle = spec.frame_middle(n);
        let started = self
            .pts
            .partition_point(|&p| Seconds::of_ticks(p, self.time_base) <= middle);
        started.saturating_sub(1)
    }

    /// The rate of the constant-rate output (plan 8 M4, review decision): the lowest of the
    /// standard rates and the source's nominal and average rates at which every source frame
    /// shows in at least one output frame. Of 29.97 and 30 (and the other NTSC pairs) both doing
    /// so, the one the source names as its nominal rate, else the one nearer its average. Where
    /// no rate up to 240 shows every frame (frames milliseconds apart), the lowest that shows the
    /// most.
    pub fn constant_rate(
        &self,
        start: Seconds,
        nominal: Option<Rational>,
        average: Option<Rational>,
    ) -> Rational {
        let (fastest_num, fastest_den) = STANDARD_RATES[STANDARD_RATES.len() - 1];
        let fastest = Rational::new(fastest_num, fastest_den).expect("a positive rate");
        let mut candidates: Vec<Rational> = STANDARD_RATES
            .iter()
            .map(|&(num, den)| Rational::new(num, den).expect("a positive rate"))
            .chain(nominal)
            .chain(average)
            .filter(|&rate| rate <= fastest)
            .collect();
        candidates.sort();
        candidates.dedup();
        let every = self.pts.len();
        let shows_every = |rate| self.frames_shown(start, rate, true) == Some(every);
        let (most, lowest) = match candidates.iter().find(|&&rate| shows_every(rate)) {
            Some(&rate) => (every, rate),
            None => {
                let counts: Vec<usize> = candidates
                    .iter()
                    .map(|&rate| self.frames_shown(start, rate, false).unwrap_or(0))
                    .collect();
                let most = counts.iter().copied().max().expect("standard rates");
                let lowest = counts.iter().position(|&n| n == most).expect("a maximum");
                (most, candidates[lowest])
            }
        };
        let Some(twin) = integer_twin(lowest) else {
            return lowest;
        };
        if self.frames_shown(start, twin, false) != Some(most) {
            return lowest;
        }
        let twin_nearer = match nominal {
            Some(rate) if rate == twin => true,
            Some(rate) if rate == lowest => false,
            _ => average.is_some_and(|average| {
                (average.as_f64() - twin.as_f64()).abs()
                    < (average.as_f64() - lowest.as_f64()).abs()
            }),
        };
        if twin_nearer { twin } else { lowest }
    }

    /// How many source frames show in at least one output frame at `rate` from `start`. With
    /// `stop_at_missing` the count stops at the first frame that shows in none. `None` where the
    /// arithmetic would overflow.
    fn frames_shown(&self, start: Seconds, rate: Rational, stop_at_missing: bool) -> Option<usize> {
        let slots = self.first_slot_from(self.end, start, rate)?.max(0);
        let mut shown = 0;
        // Output frames before the first source frame starts show it as well.
        let mut from = 0;
        for k in 0..self.pts.len() {
            let to = match self.pts.get(k + 1) {
                Some(&next) => self.first_slot_from(next, start, rate)?.clamp(0, slots),
                None => slots,
            };
            if to > from {
                shown += 1;
            } else if stop_at_missing {
                return Some(shown);
            }
            from = to;
        }
        Some(shown)
    }

    /// The first output frame, on the timeline at `rate` from `start`, whose middle comes at or
    /// after `ticks`: the least n with `rate * (ticks * time_base - start) - 1/2 <= n`. Negative
    /// before the timeline starts.
    fn first_slot_from(&self, ticks: i64, start: Seconds, rate: Rational) -> Option<i128> {
        let (tb_num, tb_den) = (
            i128::from(self.time_base.num()),
            i128::from(self.time_base.den()),
        );
        let (rate_num, rate_den) = (i128::from(rate.num()), i128::from(rate.den()));
        let start_den = i128::try_from(start.den()).ok()?;
        let since_start = i128::from(ticks)
            .checked_mul(tb_num)?
            .checked_mul(start_den)?
            .checked_sub(start.num().checked_mul(tb_den)?)?;
        let unit = tb_den.checked_mul(start_den)?.checked_mul(rate_den)?;
        let p = since_start
            .checked_mul(rate_num)?
            .checked_mul(2)?
            .checked_sub(unit)?;
        let q = unit.checked_mul(2)?;
        Some(p.div_euclid(q) + i128::from(p.rem_euclid(q) != 0))
    }

    /// [`Self::shown_at`] for every output frame in order, in one pass.
    pub fn shown(&self, spec: &TimelineSpec) -> impl Iterator<Item = usize> + '_ {
        let spec = spec.clone();
        let mut k = 0;
        (0..self.output_frames(&spec)).map(move |n| {
            let middle = spec.frame_middle(n);
            while k + 1 < self.pts.len() && self.time(k + 1) <= middle {
                k += 1;
            }
            k
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(rate: Rational, start: Seconds) -> TimelineSpec {
        TimelineSpec {
            stream: 0,
            codec: VideoCodec::H264,
            coded_width: 640,
            coded_height: 360,
            rotation: Rotation::Quarter,
            sample_aspect: Rational::new(1, 1).unwrap(),
            rate,
            nominal_rate: Some(rate),
            average_rate: Some(rate),
            variable_rate: false,
            start,
            time_base: Rational::new(1, 90_000).unwrap(),
            container_start_seconds: 0.0,
            duration_seconds: Some(3.003),
            estimated_frames: 90,
            color: ColorTags::default().complete(640, 360),
            audio: Vec::new(),
            other_video_streams: 0,
        }
    }

    #[test]
    fn frames_start_at_exact_multiples_of_the_rate_after_the_start() {
        let ntsc = Rational::new(30000, 1001).unwrap();
        let s = spec(ntsc, Seconds::new(5, 4).unwrap());
        assert_eq!(s.frame_start(0), Seconds::new(5, 4).unwrap());
        assert_eq!(
            s.frame_start(1),
            Seconds::new(5 * 7500 + 1001, 30000).unwrap()
        );
        assert_eq!(
            s.frame_start(3_000_000),
            Seconds::new(37_500 + 3_003_000_000, 30000).unwrap()
        );
        let mut later = Seconds::ZERO;
        for n in 0..100 {
            let t = s.frame_start(n);
            assert!(n == 0 || t.as_f64() > later.as_f64());
            later = t;
        }
    }

    #[test]
    fn rationals_and_seconds_are_reduced() {
        assert_eq!(Rational::new(48000, 2002), Rational::new(24000, 1001));
        assert_eq!(Rational::new(0, 1), None);
        assert_eq!(Rational::new(1, 0), None);
        assert_eq!(Seconds::new(-6, 4), Seconds::new(-3, 2));
        assert_eq!(Seconds::new(0, 7), Some(Seconds::ZERO));
        assert_eq!(Seconds::new(1, 0), None);
    }

    #[test]
    fn rotation_swaps_the_displayed_sides() {
        let s = spec(Rational::new(30, 1).unwrap(), Seconds::ZERO);
        assert_eq!(s.display_size(), (360, 640));
    }

    #[test]
    fn untagged_fields_follow_the_frame_size() {
        let hd = ColorTags::default().complete(1280, 720);
        assert_eq!(
            (hd.matrix, hd.primaries, hd.transfer, hd.range, hd.assumed),
            (
                Matrix::Bt709,
                Primaries::Bt709,
                Transfer::Bt709,
                Range::Tv,
                true
            )
        );
        for (width, height) in [(1280, 544), (960, 720), (1920, 800)] {
            assert_eq!(
                ColorTags::default().complete(width, height).matrix,
                Matrix::Bt709,
                "{width}x{height}"
            );
        }
        for (width, height) in [(720, 576), (1024, 576), (640, 480)] {
            let sd = ColorTags::default().complete(width, height);
            assert_eq!(
                (sd.matrix, sd.primaries, sd.transfer),
                (Matrix::Smpte170m, Primaries::Smpte170m, Transfer::Smpte170m),
                "{width}x{height}"
            );
        }
        let partly = ColorTags {
            matrix: Some(Matrix::Bt470bg),
            range: Some(Range::Pc),
            ..ColorTags::default()
        }
        .complete(1920, 1080);
        assert_eq!(
            (
                partly.matrix,
                partly.primaries,
                partly.range,
                partly.assumed
            ),
            (Matrix::Bt470bg, Primaries::Bt709, Range::Pc, true)
        );
        let full = ColorTags {
            matrix: Some(Matrix::Bt709),
            primaries: Some(Primaries::Bt709),
            transfer: Some(Transfer::Bt709),
            range: Some(Range::Tv),
        }
        .complete(640, 480);
        assert!(!full.assumed);
    }

    #[test]
    fn ffmpeg_names_round_trip() {
        for m in [Matrix::Bt709, Matrix::Smpte170m, Matrix::Bt2020nc] {
            assert_eq!(Matrix::from_ffmpeg_name(m.ffmpeg_name()), Some(m));
        }
        assert_eq!(
            Transfer::from_ffmpeg_name("iec61966-2-1"),
            Some(Transfer::Iec61966_2_1)
        );
        // ffprobe names transfers 4 and 5 like the primaries; the older option names still read.
        assert_eq!(
            Transfer::from_ffmpeg_name("bt470bg"),
            Some(Transfer::Bt470bg)
        );
        assert_eq!(
            Transfer::from_ffmpeg_name("gamma28"),
            Some(Transfer::Bt470bg)
        );
        assert_eq!(Transfer::Bt470m.ffmpeg_name(), "bt470m");
        assert_eq!(
            Primaries::from_ffmpeg_name("smpte432"),
            Some(Primaries::Smpte432)
        );
        assert_eq!(Matrix::from_ffmpeg_name("ycgco"), None);
        assert_eq!(Transfer::from_ffmpeg_name("smpte2084"), None);
        assert_eq!(Range::from_ffmpeg_name("unknown"), None);
    }

    fn tb(den: u32) -> Rational {
        Rational::new(1, den).unwrap()
    }

    fn at(rate: Rational, start: Seconds, time_base: Rational) -> TimelineSpec {
        TimelineSpec {
            time_base,
            ..spec(rate, start)
        }
    }

    fn every_shown(frames: &SourceFrames, s: &TimelineSpec) -> Vec<usize> {
        let one_by_one: Vec<_> = (0..frames.output_frames(s))
            .map(|n| frames.shown_at(s, n))
            .collect();
        assert_eq!(frames.shown(s).collect::<Vec<_>>(), one_by_one);
        one_by_one
    }

    #[test]
    fn a_constant_rate_source_maps_one_to_one() {
        let ticks = tb(30000);
        let pts: Vec<i64> = (0..90).map(|k| 37_500 + k * 1001).rev().collect();
        let frames = SourceFrames::new(ticks, pts, Some(1001)).unwrap();
        let s = at(
            Rational::new(30000, 1001).unwrap(),
            Seconds::new(5, 4).unwrap(),
            ticks,
        );
        assert_eq!(frames.output_frames(&s), 90);
        assert_eq!(every_shown(&frames, &s), (0..90).collect::<Vec<_>>());
        assert_eq!(frames.time(1), Seconds::new(38_501, 30000).unwrap());
    }

    #[test]
    fn a_variable_rate_source_shows_every_frame_and_repeats_the_slow_ones() {
        let ticks = tb(15360);
        let mut pts: Vec<i64> = (0..30).map(|k| k * 512).collect();
        pts.extend((1..=30).map(|k| 29 * 512 + k * 1024));
        let frames = SourceFrames::new(ticks, pts, Some(1024)).unwrap();
        let s = at(Rational::new(30, 1).unwrap(), Seconds::ZERO, ticks);
        let shown = every_shown(&frames, &s);
        let mut distinct = shown.clone();
        distinct.dedup();
        assert_eq!(distinct, (0..60).collect::<Vec<_>>());
        assert!(shown.len() > 60);
        assert!(shown.windows(2).all(|w| w[1] == w[0] || w[1] == w[0] + 1));
    }

    /// Timestamps a little off the nominal grid still give each frame its own output frame.
    #[test]
    fn jittered_timestamps_keep_every_frame() {
        let ticks = tb(90_000);
        let pts: Vec<i64> = (0..300)
            .map(|k| k * 3000 + [0, 40, -40, 15][k as usize % 4])
            .collect();
        let frames = SourceFrames::new(ticks, pts, None).unwrap();
        let s = at(Rational::new(30, 1).unwrap(), Seconds::ZERO, ticks);
        assert_eq!(every_shown(&frames, &s), (0..300).collect::<Vec<_>>());
    }

    #[test]
    fn frames_before_the_start_show_only_until_a_later_one_starts() {
        let ticks = tb(1000);
        let frames = SourceFrames::new(ticks, vec![-40, 0, 40, 80], Some(40)).unwrap();
        let s = at(Rational::new(25, 1).unwrap(), Seconds::ZERO, ticks);
        assert_eq!(every_shown(&frames, &s), [1, 2, 3]);
        let late = at(
            Rational::new(25, 1).unwrap(),
            Seconds::new(1, 1).unwrap(),
            ticks,
        );
        assert_eq!(frames.output_frames(&late), 0);
    }

    #[test]
    fn frame_lists_need_distinct_timestamps() {
        assert_eq!(
            SourceFrames::new(tb(1000), vec![0, 40, 40], None),
            Err(SourceFramesError::SameTime)
        );
        assert_eq!(
            SourceFrames::new(tb(1000), Vec::new(), None),
            Err(SourceFramesError::Empty)
        );
        let one = SourceFrames::new(tb(1000), vec![0], None).unwrap();
        let s = at(Rational::new(1000, 1).unwrap(), Seconds::ZERO, tb(1000));
        assert_eq!(one.output_frames(&s), 1);
    }

    fn rational(num: u32, den: u32) -> Rational {
        Rational::new(num, den).unwrap()
    }

    fn shows_every_frame(frames: &SourceFrames, rate: Rational) -> bool {
        let s = at(rate, Seconds::ZERO, frames.time_base);
        let mut shown: Vec<usize> = frames.shown(&s).collect();
        shown.dedup();
        shown == (0..frames.len()).collect::<Vec<_>>()
    }

    /// Ten seconds at 15 fps, then one at 30: FFmpeg guesses 15/1 from the start.
    #[test]
    fn the_output_rate_is_the_lowest_that_shows_every_frame() {
        let ticks = tb(15360);
        let mut pts: Vec<i64> = (0..150).map(|k| k * 1024).collect();
        pts.extend((0..30).map(|k| 150 * 1024 + k * 512));
        let frames = SourceFrames::new(ticks, pts, Some(512)).unwrap();
        let rate = frames.constant_rate(
            Seconds::ZERO,
            Some(rational(15, 1)),
            Some(rational(180, 11)),
        );
        assert_eq!(rate, rational(30000, 1001));
        assert!(shows_every_frame(&frames, rate));
        assert!(!shows_every_frame(&frames, rational(15, 1)));
    }

    /// Timestamps FFmpeg reads as 200/1 and 120/1 (timescale 600) are 30 and 24–30 fps.
    #[test]
    fn jitter_and_mixed_rates_keep_their_real_rate() {
        let ticks = tb(600);
        let stepped = |steps: &dyn Fn(usize) -> i64, n: usize| {
            let mut t = 0;
            (0..n)
                .map(|k| {
                    let at = t;
                    t += steps(k);
                    at
                })
                .collect::<Vec<i64>>()
        };
        let jitter =
            SourceFrames::new(ticks, stepped(&|k| [21, 21, 18][k % 3], 300), Some(20)).unwrap();
        let rate = jitter.constant_rate(
            Seconds::ZERO,
            Some(rational(200, 1)),
            Some(rational(180_000, 5999)),
        );
        assert_eq!(rate, rational(30, 1));
        assert!(shows_every_frame(&jitter, rate));

        let mixed = SourceFrames::new(
            ticks,
            stepped(&|k| if k < 60 { 20 } else { 25 }, 132),
            Some(25),
        )
        .unwrap();
        let rate = mixed.constant_rate(
            Seconds::ZERO,
            Some(rational(120, 1)),
            Some(rational(132, 5)),
        );
        assert_eq!(rate, rational(30000, 1001));
        assert!(shows_every_frame(&mixed, rate));
    }

    /// 29.97 fps slots show all of a short 30 fps clip too; the source's own rate wins.
    #[test]
    fn constant_rate_sources_keep_their_rate() {
        for (num, den, count) in [
            (24000, 1001, 72),
            (30000, 1001, 90),
            (30, 1, 100),
            (15, 1, 45),
        ] {
            let frames = SourceFrames::new(
                tb(num),
                (0..count).map(|k| k * i64::from(den)).collect(),
                Some(i64::from(den)),
            )
            .unwrap();
            let own = rational(num, den);
            assert_eq!(
                frames.constant_rate(Seconds::ZERO, Some(own), Some(own)),
                own
            );
        }
    }

    /// One frame a millisecond after another shows at no rate up to 240.
    #[test]
    fn frames_too_close_for_any_rate_take_the_lowest_rate_that_shows_the_most() {
        let mut pts: Vec<i64> = (0..60).map(|k| k * 1000 / 30).collect();
        pts.push(67);
        let frames = SourceFrames::new(tb(1000), pts, Some(33)).unwrap();
        let rate =
            frames.constant_rate(Seconds::ZERO, Some(rational(30, 1)), Some(rational(30, 1)));
        assert_eq!(rate, rational(30, 1));
        assert!(!shows_every_frame(&frames, rational(240, 1)));

        // Two pairs a millisecond apart: 144 fps shows one more of them than 240 does.
        let mut pts: Vec<i64> = (0..30).map(|k| (k * 1000 + 15) / 30).collect();
        pts.extend([1, 2, 17, 18]);
        let frames = SourceFrames::new(tb(1000), pts, Some(33)).unwrap();
        let rate =
            frames.constant_rate(Seconds::ZERO, Some(rational(30, 1)), Some(rational(30, 1)));
        assert_eq!(rate, rational(144, 1));
    }

    #[test]
    fn seconds_compare_across_denominators() {
        assert!(Seconds::new(1, 3).unwrap() < Seconds::new(34, 100).unwrap());
        assert!(Seconds::new(-1, 2).unwrap() < Seconds::ZERO);
        assert_eq!(
            Seconds::of_ticks(37_500, tb(30000)),
            Seconds::new(5, 4).unwrap()
        );
    }
}
