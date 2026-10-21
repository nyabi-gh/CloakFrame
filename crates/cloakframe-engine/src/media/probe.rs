//! ffprobe's report → [`TimelineSpec`], with the input limits (plan 4.7) and the formats this
//! release takes (plan 1.1: MP4/MOV/M4V/WebM, H.264/HEVC/VP8/VP9, 8-bit SDR).

use std::fmt;
use std::path::Path;

use cloakframe_core::timeline::{
    AudioStream, ColorTags, Matrix, Primaries, Range, Rational, Rotation, Seconds, TimelineSpec,
    Transfer, VideoCodec,
};
use serde::Deserialize;

use crate::cancel::CancelToken;
use crate::media::process::{ToolCommand, ToolError};

pub const MAX_SIDE: u32 = 16_384;
pub const MAX_PIXELS: u64 = 36_000_000;
pub const MAX_RATE: f64 = 240.0;
pub const MAX_DURATION_SECONDS: f64 = 86_400.0;
pub const MAX_FRAMES: u64 = 3_000_000;

#[derive(Debug)]
pub enum ProbeError {
    Tool(ToolError),
    /// ffprobe's output is not the report this parser reads.
    Malformed(String),
    Unsupported(Unsupported),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unsupported {
    NoVideo,
    Container(String),
    Codec(String),
    PixelFormat(String),
    Hdr(String),
    /// The display matrix mirrors the picture.
    Mirrored,
    /// A colour tag this build cannot read or decode with, rather than one it would guess.
    Color(String),
    Rotation,
    Unreadable,
    Resolution,
    FrameRate,
    Duration,
    FrameCount,
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoVideo => f.write_str("the file contains no video stream"),
            Self::Container(name) => {
                write!(f, "unsupported container '{name}' (MP4, MOV and WebM only)")
            }
            Self::Codec(name) => write!(
                f,
                "unsupported video codec '{name}' (H.264, HEVC, VP8 and VP9 only)"
            ),
            Self::PixelFormat(format) => write!(
                f,
                "unsupported pixel format '{format}' (8 bits per sample only, for now)"
            ),
            Self::Hdr(transfer) => write!(f, "HDR video ({transfer}) is not supported yet"),
            Self::Color(name) => write!(f, "the video's colour tag '{name}' is not supported"),
            Self::Rotation => f.write_str("the video's rotation is not a multiple of 90 degrees"),
            Self::Mirrored => f.write_str("mirrored video is not supported"),
            Self::Unreadable => f.write_str("the video stream could not be read"),
            Self::Resolution => f.write_str("the video resolution exceeds the limit"),
            Self::FrameRate => f.write_str("the video frame rate exceeds the limit"),
            Self::Duration => f.write_str("the video duration exceeds the limit"),
            Self::FrameCount => f.write_str("the video frame count exceeds the limit"),
        }
    }
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tool(e) => write!(f, "cannot inspect the video: {e}"),
            Self::Malformed(e) => write!(f, "cannot read ffprobe's report: {e}"),
            Self::Unsupported(e) => write!(f, "unsupported video: {e}"),
        }
    }
}

impl std::error::Error for ProbeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Tool(e) => Some(e),
            _ => None,
        }
    }
}

/// Runs ffprobe on `input` and reads its report.
pub fn probe(
    ffprobe: &Path,
    input: &Path,
    cancel: &CancelToken,
) -> Result<TimelineSpec, ProbeError> {
    let mut command = ToolCommand::ffprobe(ffprobe);
    command
        .args(["-print_format", "json", "-show_streams", "-show_format"])
        .input_file(input);
    let report = command.output(cancel).map_err(ProbeError::Tool)?;
    parse(&report)
}

#[derive(Deserialize)]
struct Report {
    #[serde(default)]
    streams: Vec<Stream>,
    format: Option<Format>,
}

#[derive(Deserialize)]
struct Format {
    format_name: Option<String>,
    start_time: Option<String>,
    duration: Option<String>,
}

#[derive(Deserialize)]
struct Stream {
    index: u32,
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    pix_fmt: Option<String>,
    sample_aspect_ratio: Option<String>,
    r_frame_rate: Option<String>,
    avg_frame_rate: Option<String>,
    time_base: Option<String>,
    start_pts: Option<i64>,
    duration: Option<String>,
    nb_frames: Option<String>,
    color_space: Option<String>,
    color_range: Option<String>,
    color_primaries: Option<String>,
    color_transfer: Option<String>,
    #[serde(default)]
    disposition: Disposition,
    #[serde(default)]
    side_data_list: Vec<SideData>,
    #[serde(default)]
    tags: Tags,
}

#[derive(Deserialize, Default)]
struct Disposition {
    #[serde(default)]
    attached_pic: u8,
}

#[derive(Deserialize)]
struct SideData {
    rotation: Option<f64>,
    /// The nine values of a display matrix, three to a line after an offset label.
    displaymatrix: Option<String>,
}

#[derive(Deserialize, Default)]
struct Tags {
    language: Option<String>,
}

/// Reads ffprobe's `-print_format json -show_streams -show_format` report. Every value comes from
/// the input file, so nothing here trusts it beyond what it checks.
pub fn parse(report: &[u8]) -> Result<TimelineSpec, ProbeError> {
    let report: Report =
        serde_json::from_slice(report).map_err(|e| ProbeError::Malformed(e.to_string()))?;
    let unsupported = |u| Err(ProbeError::Unsupported(u));

    let mut videos = report
        .streams
        .iter()
        .filter(|s| s.codec_type.as_deref() == Some("video") && s.disposition.attached_pic == 0);
    let Some(video) = videos.next() else {
        return unsupported(Unsupported::NoVideo);
    };
    let other_video_streams = videos.count() as u32;

    let codec_name = video.codec_name.as_deref().unwrap_or("unknown");
    let format = report.format.as_ref();
    let container = format
        .and_then(|f| f.format_name.as_deref())
        .unwrap_or("unknown");
    let families: Vec<&str> = container.split(',').collect();
    let mov_family = families.iter().any(|f| matches!(*f, "mov" | "mp4"));
    let webm = families.iter().any(|f| matches!(*f, "matroska" | "webm"))
        && matches!(codec_name, "vp8" | "vp9");
    if !mov_family && !webm {
        return unsupported(Unsupported::Container(container.into()));
    }

    let (Some(width), Some(height)) = (
        video.width.filter(|&w| w > 0),
        video.height.filter(|&h| h > 0),
    ) else {
        return unsupported(Unsupported::Unreadable);
    };
    let nominal = video.r_frame_rate.as_deref().and_then(rational);
    let average = video.avg_frame_rate.as_deref().and_then(rational);
    let rate = match (nominal, average) {
        (Some(r), _) if r.as_f64() <= MAX_RATE => r,
        (_, Some(a)) => a,
        (Some(r), None) => r,
        (None, None) => return unsupported(Unsupported::Unreadable),
    };
    let variable_rate = matches!((nominal, average), (Some(r), Some(a)) if r != a);
    let time_base = video
        .time_base
        .as_deref()
        .and_then(rational)
        .ok_or(ProbeError::Unsupported(Unsupported::Unreadable))?;
    let start = match video.start_pts {
        Some(pts) if pts > 0 => Seconds::of_ticks(pts, time_base),
        _ => Seconds::ZERO,
    };
    let container_start_seconds = number(format.and_then(|f| f.start_time.as_deref()))
        .unwrap_or(0.0)
        .max(0.0);
    // WebM written as it streams has none; the frame list gives it then (`media::source`).
    let duration = number(video.duration.as_deref())
        .filter(|d| *d > 0.0)
        .or_else(|| number(format.and_then(|f| f.duration.as_deref())));

    if width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return unsupported(Unsupported::Resolution);
    }
    if rate.as_f64() > MAX_RATE {
        return unsupported(Unsupported::FrameRate);
    }
    if duration.is_some_and(|d| !(0.0..=MAX_DURATION_SECONDS).contains(&d)) {
        return unsupported(Unsupported::Duration);
    }
    let counted = video
        .nb_frames
        .as_deref()
        .and_then(|n| n.parse::<u64>().ok())
        .filter(|&n| n > 0 && !variable_rate);
    let estimated_frames = counted
        .or_else(|| duration.map(|d| (d * rate.as_f64()).ceil() as u64))
        .unwrap_or(0);
    if estimated_frames > MAX_FRAMES {
        return unsupported(Unsupported::FrameCount);
    }

    let codec = match codec_name {
        "h264" => VideoCodec::H264,
        "hevc" => VideoCodec::Hevc,
        "vp8" => VideoCodec::Vp8,
        "vp9" => VideoCodec::Vp9,
        _ => return unsupported(Unsupported::Codec(codec_name.into())),
    };
    let pixel_format = video.pix_fmt.as_deref().unwrap_or("unknown");
    if !EIGHT_BIT_FORMATS.contains(&pixel_format) {
        return unsupported(Unsupported::PixelFormat(pixel_format.into()));
    }
    let transfer_name = video.color_transfer.as_deref().unwrap_or("unknown");
    if matches!(transfer_name, "smpte2084" | "arib-std-b67") {
        return unsupported(Unsupported::Hdr(transfer_name.into()));
    }

    let rotation = rotation(video).map_err(ProbeError::Unsupported)?;
    let sample_aspect = video
        .sample_aspect_ratio
        .as_deref()
        .and_then(rational)
        .unwrap_or(Rational::new(1, 1).expect("1:1"));
    let tags = ColorTags {
        matrix: color_tag(video.color_space.as_deref(), Matrix::from_ffmpeg_name)?,
        primaries: color_tag(
            video.color_primaries.as_deref(),
            Primaries::from_ffmpeg_name,
        )?,
        transfer: color_tag(video.color_transfer.as_deref(), Transfer::from_ffmpeg_name)?,
        range: color_tag(video.color_range.as_deref(), Range::from_ffmpeg_name)?,
    };
    let audio = report
        .streams
        .iter()
        .filter(|s| s.codec_type.as_deref() == Some("audio"))
        .map(|s| AudioStream {
            index: s.index,
            codec: s.codec_name.clone().unwrap_or_else(|| "unknown".into()),
            language: s.tags.language.as_deref().and_then(language),
        })
        .collect();

    Ok(TimelineSpec {
        stream: video.index,
        codec,
        coded_width: width,
        coded_height: height,
        rotation,
        sample_aspect,
        rate,
        nominal_rate: nominal,
        average_rate: average,
        variable_rate,
        start,
        time_base,
        container_start_seconds,
        duration_seconds: duration,
        estimated_frames,
        color: tags.complete(width, height),
        audio,
        other_video_streams,
    })
}

/// A colour field as the source tagged it, `None` when untagged. A name this build does not know
/// refuses the video: taking it as untagged would decode and tag the video as something it is
/// not (D1).
fn color_tag<T>(name: Option<&str>, known: fn(&str) -> Option<T>) -> Result<Option<T>, ProbeError> {
    match name {
        None | Some("unknown" | "unspecified" | "reserved" | "reserved0") => Ok(None),
        Some(name) => known(name)
            .map(Some)
            .ok_or_else(|| ProbeError::Unsupported(Unsupported::Color(name.into()))),
    }
}

/// `30000/1001` or `16:9`; zero or negative parts are not a rate.
/// ffprobe prints each part as a C `int`; a larger one did not come from it, and would take the
/// timeline's exact arithmetic past `i128`.
fn rational(text: &str) -> Option<Rational> {
    let (num, den) = text.split_once('/').or_else(|| text.split_once(':'))?;
    let part = |p: &str| {
        p.trim()
            .parse::<i32>()
            .ok()
            .and_then(|p| u32::try_from(p).ok())
    };
    Rational::new(part(num)?, part(den)?)
}

fn number(text: Option<&str>) -> Option<f64> {
    text?.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// The 8-bit formats FFmpeg's H.264, HEVC, VP8 and VP9 decoders produce. A list rather than
/// reading a depth from the name: `nv12` and `rgb24` end in digits too.
const EIGHT_BIT_FORMATS: [&str; 14] = [
    "yuv420p", "yuvj420p", "yuv422p", "yuvj422p", "yuv444p", "yuvj444p", "yuv440p", "yuvj440p",
    "yuv411p", "yuva420p", "gray", "gbrp", "nv12", "nv21",
];

/// The turn FFmpeg gives the picture when decoding (fftools' display matrix rule): the matrix's
/// angle rounded to whole degrees, transposing only on an exact quarter turn. Any other angle
/// gets an arbitrary rotation that keeps the frame size, and a mirroring matrix a flip, neither of
/// which [`Rotation`] describes, so both refuse the video. FFmpeg no longer applies the old
/// `rotate` tag, so neither does this.
fn rotation(stream: &Stream) -> Result<Rotation, Unsupported> {
    let Some(side) = stream
        .side_data_list
        .iter()
        .rev()
        .find(|d| d.displaymatrix.is_some() || d.rotation.is_some())
    else {
        return Ok(Rotation::None);
    };
    // How far the display turns the picture counter-clockwise (av_display_rotation_get).
    let counter_clockwise = match side.displaymatrix.as_deref() {
        Some(text) => {
            let [a, b, c, d] = display_matrix(text).ok_or(Unsupported::Rotation)?;
            if a * d - b * c < 0.0 {
                return Err(Unsupported::Mirrored);
            }
            let (scale_x, scale_y) = (a.hypot(c), b.hypot(d));
            if scale_x == 0.0 || scale_y == 0.0 {
                return Err(Unsupported::Rotation);
            }
            -(b / scale_y).atan2(a / scale_x).to_degrees()
        }
        None => side.rotation.unwrap_or(0.0),
    };
    // ffprobe reports at most half a turn either way; anything else did not come from a matrix.
    if !(-360.0..=360.0).contains(&counter_clockwise) {
        return Err(Unsupported::Rotation);
    }
    match (-counter_clockwise.round()).rem_euclid(360.0) as u16 {
        0 => Ok(Rotation::None),
        90 => Ok(Rotation::Quarter),
        180 => Ok(Rotation::Half),
        270 => Ok(Rotation::ThreeQuarters),
        _ => Err(Unsupported::Rotation),
    }
}

/// The 2x2 rotation and scale part of ffprobe's display matrix text, in 16.16 fixed point.
fn display_matrix(text: &str) -> Option<[f64; 4]> {
    let values: Vec<i64> = text
        .lines()
        .filter_map(|line| line.split_once(':'))
        .flat_map(|(_, row)| row.split_whitespace().map(str::parse))
        .collect::<Result<_, _>>()
        .ok()?;
    let [a, b, _, c, d, ..] = values[..] else {
        return None;
    };
    (values.len() == 9).then(|| [a, b, c, d].map(|v| v as f64 / 65536.0))
}

fn language(tag: &str) -> Option<String> {
    ((2..=3).contains(&tag.len()) && tag.bytes().all(|b| b.is_ascii_lowercase()) && tag != "und")
        .then(|| tag.to_owned())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn video() -> Value {
        json!({
            "index": 0, "codec_type": "video", "codec_name": "h264", "width": 1920,
            "height": 1080, "pix_fmt": "yuv420p", "r_frame_rate": "30000/1001",
            "avg_frame_rate": "30000/1001", "time_base": "1/30000", "start_pts": 0,
            "duration": "10.010000", "nb_frames": "300",
            "disposition": {"default": 1, "attached_pic": 0}
        })
    }

    fn report(streams: Vec<Value>) -> Value {
        json!({"streams": streams, "format": {"format_name": "mov,mp4,m4a,3gp,3g2,mj2", "duration": "10.010000"}})
    }

    fn with(mut stream: Value, changes: Value) -> Value {
        for (k, v) in changes.as_object().unwrap() {
            stream[k] = v.clone();
        }
        stream
    }

    fn parsed(report: &Value) -> Result<TimelineSpec, ProbeError> {
        parse(report.to_string().as_bytes())
    }

    fn refused(report: &Value) -> Unsupported {
        match parsed(report) {
            Err(ProbeError::Unsupported(u)) => u,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_plain_mp4_becomes_a_spec() {
        let s = parsed(&report(vec![video()])).unwrap();
        assert_eq!(s.codec, VideoCodec::H264);
        assert_eq!((s.coded_width, s.coded_height), (1920, 1080));
        assert_eq!(s.rate, Rational::new(30000, 1001).unwrap());
        assert!(!s.variable_rate);
        assert_eq!(s.estimated_frames, 300);
        assert_eq!(s.start, Seconds::ZERO);
        assert_eq!(s.rotation, Rotation::None);
        assert_eq!(s.sample_aspect, Rational::new(1, 1).unwrap());
        assert!(s.color.assumed);
        assert_eq!(s.color.matrix, Matrix::Bt709);
    }

    #[test]
    fn cover_art_is_skipped_and_other_videos_are_counted() {
        let cover = with(
            video(),
            json!({"index": 0, "codec_name": "png", "disposition": {"attached_pic": 1}}),
        );
        let main = with(video(), json!({"index": 1}));
        let second = with(video(), json!({"index": 2}));
        let s = parsed(&report(vec![cover, main, second])).unwrap();
        assert_eq!((s.stream, s.other_video_streams), (1, 1));
    }

    #[test]
    fn a_file_without_video_is_refused() {
        let cover = with(video(), json!({"disposition": {"attached_pic": 1}}));
        let audio = json!({"index": 1, "codec_type": "audio", "codec_name": "aac"});
        assert_eq!(refused(&report(vec![cover, audio])), Unsupported::NoVideo);
    }

    #[test]
    fn containers_are_judged_by_content() {
        let mut r = report(vec![with(video(), json!({"codec_name": "vp9"}))]);
        r["format"]["format_name"] = json!("matroska,webm");
        assert!(parsed(&r).is_ok());
        r["streams"][0]["codec_name"] = json!("h264");
        assert_eq!(refused(&r), Unsupported::Container("matroska,webm".into()));
        r["format"]["format_name"] = json!("avi");
        assert_eq!(refused(&r), Unsupported::Container("avi".into()));
        r["format"] = json!({});
        assert_eq!(refused(&r), Unsupported::Container("unknown".into()));
    }

    #[test]
    fn codecs_pixel_formats_and_hdr_outside_the_release_are_refused() {
        let r = |changes| report(vec![with(video(), changes)]);
        assert_eq!(
            refused(&r(json!({"codec_name": "av1"}))),
            Unsupported::Codec("av1".into())
        );
        for format in ["yuv420p10le", "p010le", "yuv422p12be", "rgb48le"] {
            assert_eq!(
                refused(&r(json!({"pix_fmt": format}))),
                Unsupported::PixelFormat(format.into())
            );
        }
        for format in ["nv12", "yuvj420p", "yuv444p", "gbrp"] {
            assert!(parsed(&r(json!({"pix_fmt": format}))).is_ok(), "{format}");
        }
        for transfer in ["smpte2084", "arib-std-b67"] {
            assert_eq!(
                refused(&r(json!({"color_transfer": transfer}))),
                Unsupported::Hdr(transfer.into())
            );
        }
    }

    #[test]
    fn the_input_limits_are_1_12_1s() {
        let r = |changes| report(vec![with(video(), changes)]);
        assert!(parsed(&r(json!({"width": 16384, "height": 2196}))).is_ok());
        assert_eq!(
            refused(&r(json!({"width": 16385, "height": 100}))),
            Unsupported::Resolution
        );
        assert_eq!(
            refused(&r(json!({"width": 6001, "height": 6000}))),
            Unsupported::Resolution
        );
        assert_eq!(
            refused(&r(
                json!({"r_frame_rate": "241/1", "avg_frame_rate": "241/1"})
            )),
            Unsupported::FrameRate
        );
        assert_eq!(
            refused(&r(json!({"duration": "86400.5", "nb_frames": "1"}))),
            Unsupported::Duration
        );
        assert_eq!(
            refused(&r(json!({"nb_frames": "3000001"}))),
            Unsupported::FrameCount
        );
        assert_eq!(
            refused(&r(
                json!({"nb_frames": null, "duration": "50001", "r_frame_rate": "60/1", "avg_frame_rate": "60/1"})
            )),
            Unsupported::FrameCount
        );
        assert_eq!(refused(&r(json!({"width": 0}))), Unsupported::Unreadable);
        assert_eq!(
            refused(&r(json!({"time_base": "4294967295/1"}))),
            Unsupported::Unreadable
        );
        assert_eq!(
            refused(&r(json!({"r_frame_rate": "0/0", "avg_frame_rate": "0/0"}))),
            Unsupported::Unreadable
        );
    }

    #[test]
    fn unspecified_colour_is_untagged_and_an_unknown_name_is_refused() {
        let r = |changes| report(vec![with(video(), changes)]);
        let s = parsed(&r(
            json!({"color_space": "reserved", "color_transfer": "unknown"}),
        ))
        .unwrap();
        assert_eq!((s.color.matrix, s.color.assumed), (Matrix::Bt709, true));
        assert_eq!(
            refused(&r(json!({"color_primaries": "smpte999"}))),
            Unsupported::Color("smpte999".into())
        );
        assert_eq!(
            refused(&r(json!({"color_space": "ictcp"}))),
            Unsupported::Color("ictcp".into())
        );
    }

    #[test]
    fn a_report_without_a_duration_still_makes_a_spec() {
        let mut streamed = report(vec![with(
            video(),
            json!({"duration": null, "nb_frames": null}),
        )]);
        streamed["format"]["duration"] = Value::Null;
        let s = parsed(&streamed).unwrap();
        assert_eq!((s.duration_seconds, s.estimated_frames), (None, 0));
    }

    #[test]
    fn a_variable_rate_keeps_every_frame_at_the_nominal_rate() {
        let r = |changes| report(vec![with(video(), changes)]);
        let s = parsed(&r(json!({"r_frame_rate": "30/1", "avg_frame_rate": "600/29", "duration": "2.9", "nb_frames": "60"}))).unwrap();
        assert_eq!(s.rate, Rational::new(30, 1).unwrap());
        assert!(s.variable_rate);
        assert_eq!(s.estimated_frames, 87);

        let s = parsed(&r(
            json!({"r_frame_rate": "90000/1", "avg_frame_rate": "30/1"}),
        ))
        .unwrap();
        assert_eq!(s.rate, Rational::new(30, 1).unwrap());
        let s = parsed(&r(json!({"r_frame_rate": "0/0", "avg_frame_rate": "25/1"}))).unwrap();
        assert_eq!(s.rate, Rational::new(25, 1).unwrap());
    }

    #[test]
    fn the_start_is_exact_and_never_negative() {
        let r = |changes| report(vec![with(video(), changes)]);
        let s = parsed(&r(json!({"start_pts": 37500}))).unwrap();
        assert_eq!(s.start, Seconds::new(5, 4).unwrap());
        let s = parsed(&r(json!({"start_pts": -1024}))).unwrap();
        assert_eq!(s.start, Seconds::ZERO);
    }

    /// ffprobe's text for a display matrix with these 16.16 values in its first two rows.
    fn matrix(a: i64, b: i64, c: i64, d: i64) -> Value {
        json!({"side_data_list": [{
            "side_data_type": "Display Matrix",
            "displaymatrix": format!(
                "\n00000000: {a:>12} {b:>11} {:>11}\n00000001: {c:>12} {d:>11} {:>11}\n00000002: {:>12} {:>11} {:>11}\n",
                0, 0, 0, 0, 1 << 30
            ),
            "rotation": 0
        }]})
    }

    #[test]
    fn rotation_follows_the_display_matrix_as_ffmpeg_applies_it() {
        let r = |changes| report(vec![with(video(), changes)]);
        let turned = |changes| parsed(&r(changes)).unwrap().rotation;
        const ONE: i64 = 65536;
        // ffprobe's 90 (counter-clockwise), as the P-3 fixture has it.
        assert_eq!(turned(matrix(0, -ONE, ONE, 0)), Rotation::ThreeQuarters);
        assert_eq!(turned(matrix(0, ONE, -ONE, 0)), Rotation::Quarter);
        assert_eq!(turned(matrix(-ONE, 0, 0, -ONE)), Rotation::Half);
        // 90.4 degrees rounds to a quarter turn; 90.6 does not, and FFmpeg keeps the frame size.
        assert_eq!(
            turned(matrix(-457, -65534, 65534, -457)),
            Rotation::ThreeQuarters
        );
        assert_eq!(
            refused(&r(matrix(-686, -65532, 65532, -686))),
            Unsupported::Rotation
        );
        // A flip alone shows as -180 but FFmpeg only mirrors the picture.
        assert_eq!(refused(&r(matrix(-ONE, 0, 0, ONE))), Unsupported::Mirrored);
        assert_eq!(refused(&r(matrix(0, 0, 0, 0))), Unsupported::Rotation);
        // Without the matrix text, the reported angle.
        assert_eq!(
            turned(json!({"side_data_list": [{"rotation": -90}]})),
            Rotation::Quarter
        );
        assert_eq!(
            refused(&r(json!({"side_data_list": [{"rotation": 45}]}))),
            Unsupported::Rotation
        );
        assert_eq!(turned(json!({"tags": {"rotate": "270"}})), Rotation::None);
        // Found by fuzzing: a turn count past i64 overflowed when negated.
        assert_eq!(
            refused(&r(json!({"side_data_list": [{"rotation": 1e300}]}))),
            Unsupported::Rotation
        );
        assert_eq!(
            refused(&r(json!({"side_data_list": [{"rotation": -8.3e20}]}))),
            Unsupported::Rotation
        );
    }

    #[test]
    fn colour_tags_are_kept_and_the_rest_assumed() {
        let r = |changes| report(vec![with(video(), changes)]);
        let c = parsed(&r(json!({"color_space": "bt709", "color_primaries": "bt709", "color_transfer": "bt709", "color_range": "pc"}))).unwrap().color;
        assert_eq!((c.range, c.assumed), (Range::Pc, false));
        let c = parsed(&r(
            json!({"width": 640, "height": 480, "color_space": "unknown", "color_range": "tv"}),
        ))
        .unwrap()
        .color;
        assert_eq!(
            (c.matrix, c.range, c.assumed),
            (Matrix::Smpte170m, Range::Tv, true)
        );
    }

    #[test]
    fn audio_streams_keep_only_a_plain_language_code() {
        let audio = |index, language: Value| json!({"index": index, "codec_type": "audio", "codec_name": "aac", "tags": {"language": language}});
        let s = parsed(&report(vec![
            video(),
            audio(1, json!("kor")),
            audio(2, json!("und")),
            audio(3, json!("EN")),
            audio(4, json!("en-US")),
        ]))
        .unwrap();
        let languages: Vec<_> = s.audio.iter().map(|a| a.language.as_deref()).collect();
        assert_eq!(languages, [Some("kor"), None, None, None]);
        assert_eq!(s.audio[3].index, 4);
    }

    #[test]
    fn a_report_that_is_not_ffprobes_is_malformed() {
        for bad in [
            "",
            "[]",
            "{\"streams\": 3}",
            "{\"streams\": [{\"index\": -1}]}",
        ] {
            assert!(
                matches!(parse(bad.as_bytes()), Err(ProbeError::Malformed(_))),
                "{bad}"
            );
        }
    }
}
