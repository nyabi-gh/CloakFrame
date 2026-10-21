//! The one description of a video's timeline that analysis, preview and export all use (plan 4.4,
//! P-3). It is built once from ffprobe's report; frame `n` of every pass is the frame shown at
//! [`TimelineSpec::frame_start`]`(n)` on a constant-rate timeline.

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
/// assumed from the frame height and marked in [`ColorSpec::assumed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorSpec {
    pub matrix: Matrix,
    pub primaries: Primaries,
    pub transfer: Transfer,
    pub range: Range,
    pub assumed: bool,
}

/// The names are FFmpeg's, as ffprobe reports them and the encoder's colour options take them.
macro_rules! ffmpeg_names {
    ($(#[$doc:meta])* $name:ident { $($variant:ident = $text:literal),+ $(,)? }) => {
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
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

ffmpeg_names!(Matrix {
    Bt709 = "bt709",
    Bt470bg = "bt470bg",
    Smpte170m = "smpte170m",
    Smpte240m = "smpte240m",
    Fcc = "fcc",
    Bt2020nc = "bt2020nc",
});

ffmpeg_names!(Primaries {
    Bt709 = "bt709",
    Bt470m = "bt470m",
    Bt470bg = "bt470bg",
    Smpte170m = "smpte170m",
    Smpte240m = "smpte240m",
    Film = "film",
    Bt2020 = "bt2020",
});

ffmpeg_names!(
    /// SDR transfers only; PQ and HLG are refused before a spec is made (until M9).
    Transfer {
        Bt709 = "bt709",
        Gamma22 = "gamma22",
        Gamma28 = "gamma28",
        Smpte170m = "smpte170m",
        Smpte240m = "smpte240m",
        Linear = "linear",
        Iec61966_2_1 = "iec61966-2-1",
        Iec61966_2_4 = "iec61966-2-4",
        Bt2020_10 = "bt2020-10",
        Bt2020_12 = "bt2020-12",
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

/// Frames this tall or taller are taken as HD (BT.709) when untagged, smaller ones as SD
/// (SMPTE 170M), in limited range (the D1 fixture's rule).
pub const HD_MIN_HEIGHT: u32 = 720;

impl ColorTags {
    pub fn complete(self, height: u32) -> ColorSpec {
        let hd = height >= HD_MIN_HEIGHT;
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
    /// An ISO 639 code of two or three lowercase letters; `und` and anything else is dropped.
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
    /// The constant rate of every pass. A variable-rate source gets its nominal rate, so no
    /// source frame is dropped and slower stretches repeat frames.
    pub rate: Rational,
    pub variable_rate: bool,
    /// When the first video frame is shown, never before zero.
    pub start: Seconds,
    pub duration_seconds: f64,
    /// For limits and progress only; pass 1 counts the frames.
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
        let rate_num = u128::from(self.rate.num());
        let rate_den = u128::from(self.rate.den());
        let den = self.start.den() * rate_num;
        let num = self.start.num() * rate_num as i128
            + (u128::from(n) * rate_den * self.start.den()) as i128;
        Seconds::new(num, den).expect("a nonzero denominator")
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
            variable_rate: false,
            start,
            duration_seconds: 3.003,
            estimated_frames: 90,
            color: ColorTags::default().complete(360),
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
    fn untagged_fields_follow_the_frame_height() {
        let hd = ColorTags::default().complete(720);
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
        let sd = ColorTags::default().complete(719);
        assert_eq!(
            (sd.matrix, sd.primaries, sd.transfer),
            (Matrix::Smpte170m, Primaries::Smpte170m, Transfer::Smpte170m)
        );
        let partly = ColorTags {
            matrix: Some(Matrix::Bt470bg),
            range: Some(Range::Pc),
            ..ColorTags::default()
        }
        .complete(1080);
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
        .complete(480);
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
        assert_eq!(Transfer::from_ffmpeg_name("smpte2084"), None);
        assert_eq!(Range::from_ffmpeg_name("unknown"), None);
    }
}
