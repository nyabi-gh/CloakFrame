//! The encoded output checked before it is published (P-4): its streams, size, colour tags,
//! duration and metadata from ffprobe, then a full decode that must end without an error and
//! give every frame. 1.12.1 published on the encoder's exit status alone.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use cloakframe_core::timeline::{Matrix, Primaries, Range, Rational, TimelineSpec, Transfer};
use serde::Deserialize;

use crate::cancel::CancelToken;
use crate::media::encode::{OutputCodec, encoded_size};
use crate::media::process::{ToolCommand, ToolError};

/// Tags the MP4 muxer and encoders write themselves once the source's metadata is dropped
/// (`-map_metadata -1`). Anything else came from somewhere it should not have.
const FORMAT_TAGS: [&str; 4] = [
    "major_brand",
    "minor_version",
    "compatible_brands",
    "encoder",
];
const STREAM_TAGS: [&str; 4] = ["language", "handler_name", "vendor_id", "encoder"];

/// What the output must be.
#[derive(Debug, Clone, PartialEq)]
pub struct Expected {
    pub codec: OutputCodec,
    pub size: (u32, u32),
    pub frames: u64,
    pub rate: Rational,
    pub matrix: Matrix,
    pub primaries: Primaries,
    pub transfer: Transfer,
    pub range: Range,
    pub audio: Vec<ExpectedAudio>,
}

/// An audio stream of the output, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpectedAudio {
    pub language: Option<String>,
    /// The source's sound lasts to the video's end, so the output's must too.
    pub to_the_end: bool,
}

impl Expected {
    /// The output [`FrameWriter`](crate::media::encode::FrameWriter) makes of `spec` with
    /// `frames` frames and the source's audio.
    pub fn of(spec: &TimelineSpec, codec: OutputCodec, frames: u64) -> Self {
        let seconds = frames as f64 * f64::from(spec.rate.den()) / f64::from(spec.rate.num());
        Self {
            codec,
            size: encoded_size(spec.display_size()),
            frames,
            rate: spec.rate,
            matrix: spec.output_matrix(),
            primaries: spec.color.primaries,
            transfer: spec.color.transfer,
            range: spec.color.range,
            audio: spec
                .audio
                .iter()
                .map(|a| ExpectedAudio {
                    language: a.language.clone(),
                    to_the_end: a
                        .end
                        .is_some_and(|end| end.as_f64() - spec.start.as_f64() >= seconds),
                })
                .collect(),
        }
    }

    fn frame_seconds(&self) -> f64 {
        f64::from(self.rate.den()) / f64::from(self.rate.num())
    }

    fn seconds(&self) -> f64 {
        self.frames as f64 * self.frame_seconds()
    }
}

#[derive(Debug)]
pub enum VerifyError {
    Tool(ToolError),
    Malformed(String),
    /// Not one video stream and the expected audio streams, or chapters.
    Streams(String),
    Video(String),
    Duration {
        stream: String,
        expected: f64,
        found: Option<f64>,
    },
    Metadata(String),
    /// The full decode stopped on an error.
    Decode(ToolError),
    FrameCount {
        expected: u64,
        decoded: u64,
    },
    Cancelled,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the encoded video failed its check: ")?;
        match self {
            Self::Tool(e) => write!(f, "cannot inspect it: {e}"),
            Self::Malformed(e) => write!(f, "cannot read ffprobe's report: {e}"),
            Self::Streams(why) | Self::Video(why) => f.write_str(why),
            Self::Duration {
                stream,
                expected,
                found: Some(found),
            } => write!(f, "{stream} lasts {found:.3} s instead of {expected:.3} s"),
            Self::Duration { stream, .. } => write!(f, "{stream} has no duration"),
            Self::Metadata(key) => write!(f, "it carries the metadata '{key}'"),
            Self::Decode(e) => write!(f, "it does not decode: {e}"),
            Self::FrameCount { expected, decoded } => {
                write!(f, "{decoded} frames decode instead of {expected}")
            }
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for VerifyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Tool(e) | Self::Decode(e) => Some(e),
            _ => None,
        }
    }
}

pub fn check(
    ffprobe: &Path,
    ffmpeg: &Path,
    output: &Path,
    expected: &Expected,
    cancel: &CancelToken,
) -> Result<(), VerifyError> {
    let mut command = ToolCommand::ffprobe(ffprobe);
    command
        .args([
            "-print_format",
            "json",
            "-show_streams",
            "-show_format",
            "-show_chapters",
        ])
        .input_file(output);
    let report = command.output(cancel).map_err(|e| match e {
        ToolError::Cancelled => VerifyError::Cancelled,
        e => VerifyError::Tool(e),
    })?;
    check_report(&report, expected)?;
    let decoded = decode_all(ffmpeg, output, cancel)?;
    if decoded != expected.frames {
        return Err(VerifyError::FrameCount {
            expected: expected.frames,
            decoded,
        });
    }
    Ok(())
}

#[derive(Deserialize)]
struct Report {
    #[serde(default)]
    streams: Vec<Stream>,
    format: Option<Format>,
    #[serde(default)]
    chapters: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct Format {
    #[serde(default)]
    tags: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct Stream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    color_space: Option<String>,
    color_range: Option<String>,
    color_primaries: Option<String>,
    color_transfer: Option<String>,
    start_time: Option<String>,
    duration: Option<String>,
    #[serde(default)]
    tags: BTreeMap<String, String>,
}

fn check_report(report: &[u8], expected: &Expected) -> Result<(), VerifyError> {
    let report: Report =
        serde_json::from_slice(report).map_err(|e| VerifyError::Malformed(e.to_string()))?;
    let kinds: Vec<&str> = report
        .streams
        .iter()
        .map(|s| s.codec_type.as_deref().unwrap_or("unknown"))
        .collect();
    let mut wanted = vec!["video"];
    wanted.extend(expected.audio.iter().map(|_| "audio"));
    if kinds != wanted {
        return Err(VerifyError::Streams(format!(
            "its streams are [{}] instead of [{}]",
            kinds.join(", "),
            wanted.join(", ")
        )));
    }
    if !report.chapters.is_empty() {
        return Err(VerifyError::Streams("it has chapters".to_owned()));
    }
    let (video, audio) = report.streams.split_first().expect("one video stream");
    check_video(video, expected)?;

    // A frame either way: the muxer rounds the last frame's duration to its time base.
    let tolerance = expected.frame_seconds() * 1.001;
    let video_seconds = seconds(video);
    if !video_seconds.is_some_and(|s| (s - expected.seconds()).abs() <= tolerance) {
        return Err(VerifyError::Duration {
            stream: "the video".to_owned(),
            expected: expected.seconds(),
            found: video_seconds,
        });
    }
    for (i, (stream, kept)) in audio.iter().zip(&expected.audio).enumerate() {
        // Audio may end before the video when the source's does, never after it.
        let found = seconds(stream);
        let too_long = !found.is_some_and(|s| s <= expected.seconds() + tolerance);
        let end = found.map(|s| start_time(stream) + s - start_time(video));
        let too_short =
            kept.to_the_end && !end.is_some_and(|e| e >= expected.seconds() - tolerance);
        if too_long || too_short {
            return Err(VerifyError::Duration {
                stream: format!("audio stream {}", i + 1),
                expected: expected.seconds(),
                found,
            });
        }
        let language = &kept.language;
        let tagged = stream.tags.get("language").map(String::as_str);
        let tagged = tagged.filter(|&l| l != "und");
        if tagged != language.as_deref() {
            return Err(VerifyError::Streams(format!(
                "audio stream {} has the language {} instead of {}",
                i + 1,
                tagged.unwrap_or("none"),
                language.as_deref().unwrap_or("none")
            )));
        }
    }

    let format_tags = report.format.map(|f| f.tags).unwrap_or_default();
    let stray = format_tags
        .keys()
        .find(|k| !FORMAT_TAGS.contains(&k.as_str()))
        .or_else(|| {
            report
                .streams
                .iter()
                .flat_map(|s| s.tags.keys())
                .find(|k| !STREAM_TAGS.contains(&k.as_str()))
        });
    match stray {
        Some(key) => Err(VerifyError::Metadata(key.clone())),
        None => Ok(()),
    }
}

fn check_video(video: &Stream, expected: &Expected) -> Result<(), VerifyError> {
    let codec = match expected.codec {
        OutputCodec::H264 => "h264",
        OutputCodec::Hevc => "hevc",
    };
    if video.codec_name.as_deref() != Some(codec) {
        return Err(VerifyError::Video(format!(
            "the video is {} instead of {codec}",
            video.codec_name.as_deref().unwrap_or("of no known codec")
        )));
    }
    let size = (video.width.unwrap_or(0), video.height.unwrap_or(0));
    if size != expected.size {
        return Err(VerifyError::Video(format!(
            "the video is {}×{} instead of {}×{}",
            size.0, size.1, expected.size.0, expected.size.1
        )));
    }
    // Compared as values: ffprobe and the encoder options name a few of them differently, and
    // the core enums read both names.
    let tags = [
        (
            "matrix",
            &video.color_space,
            expected.matrix.ffmpeg_name(),
            video
                .color_space
                .as_deref()
                .and_then(Matrix::from_ffmpeg_name)
                == Some(expected.matrix),
        ),
        (
            "primaries",
            &video.color_primaries,
            expected.primaries.ffmpeg_name(),
            video
                .color_primaries
                .as_deref()
                .and_then(Primaries::from_ffmpeg_name)
                == Some(expected.primaries),
        ),
        (
            "transfer",
            &video.color_transfer,
            expected.transfer.ffmpeg_name(),
            video
                .color_transfer
                .as_deref()
                .and_then(Transfer::from_ffmpeg_name)
                == Some(expected.transfer),
        ),
        (
            "range",
            &video.color_range,
            expected.range.ffmpeg_name(),
            video
                .color_range
                .as_deref()
                .and_then(Range::from_ffmpeg_name)
                == Some(expected.range),
        ),
    ];
    for (what, found, wanted, same) in tags {
        if !same {
            return Err(VerifyError::Video(format!(
                "the video's {what} is tagged {} instead of {wanted}",
                found.as_deref().unwrap_or("nothing")
            )));
        }
    }
    Ok(())
}

fn seconds(stream: &Stream) -> Option<f64> {
    stream
        .duration
        .as_deref()?
        .parse()
        .ok()
        .filter(|s: &f64| s.is_finite())
}

fn start_time(stream: &Stream) -> f64 {
    stream
        .start_time
        .as_deref()
        .and_then(|s| s.parse().ok())
        .filter(|s: &f64| s.is_finite())
        .unwrap_or(0.0)
}

/// Every stream decoded to nothing; the number of video frames the decode gave. `-xerror` makes
/// a damaged packet fail the decode instead of being skipped: without it a cut-off file decodes
/// with exit status 0, one frame short.
fn decode_all(ffmpeg: &Path, output: &Path, cancel: &CancelToken) -> Result<u64, VerifyError> {
    let mut command = ToolCommand::ffmpeg(ffmpeg);
    command
        .args(["-nostats", "-xerror"])
        .input_file(output)
        .args(["-map", "0", "-fps_mode", "passthrough", "-f", "null"])
        .args(["-progress", "pipe:1", "-"]);
    let progress = command.output(cancel).map_err(|e| match e {
        ToolError::Cancelled => VerifyError::Cancelled,
        e => VerifyError::Decode(e),
    })?;
    String::from_utf8_lossy(&progress)
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("frame="))
        .and_then(|n| n.trim().parse().ok())
        .ok_or_else(|| VerifyError::Malformed("the decode reported no frame count".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn expected() -> Expected {
        Expected {
            codec: OutputCodec::H264,
            size: (642, 362),
            frames: 60,
            rate: Rational::new(30, 1).unwrap(),
            matrix: Matrix::Smpte170m,
            primaries: Primaries::Bt470bg,
            transfer: Transfer::Bt470bg,
            range: Range::Tv,
            audio: vec![ExpectedAudio {
                language: Some("eng".to_owned()),
                to_the_end: false,
            }],
        }
    }

    fn report() -> serde_json::Value {
        json!({
            "streams": [
                {
                    "codec_type": "video", "codec_name": "h264", "width": 642, "height": 362,
                    "color_space": "smpte170m", "color_range": "tv",
                    "color_primaries": "bt470bg", "color_transfer": "bt470bg",
                    "duration": "2.000000",
                    "tags": {"language": "und", "handler_name": "VideoHandler",
                             "vendor_id": "[0][0][0][0]", "encoder": "Lavc libx264"}
                },
                {
                    "codec_type": "audio", "codec_name": "aac", "duration": "1.990000",
                    "tags": {"language": "eng", "handler_name": "SoundHandler"}
                }
            ],
            "format": {"tags": {"major_brand": "isom", "minor_version": "512",
                                "compatible_brands": "isomiso2avc1mp41", "encoder": "Lavf"}},
            "chapters": []
        })
    }

    fn checked(report: &serde_json::Value) -> Result<(), VerifyError> {
        check_report(report.to_string().as_bytes(), &expected())
    }

    #[test]
    fn what_the_writer_makes_passes() {
        checked(&report()).unwrap();
    }

    #[test]
    fn each_difference_from_the_expected_output_fails() {
        type Change = fn(&mut serde_json::Value);
        let changes: [(&str, Change); 11] = [
            ("extra stream", |r| {
                r["streams"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"codec_type": "data"}))
            }),
            ("no audio", |r| {
                r["streams"].as_array_mut().unwrap().pop();
            }),
            ("chapters", |r| r["chapters"] = json!([{"id": 0}])),
            ("codec", |r| r["streams"][0]["codec_name"] = json!("hevc")),
            ("size", |r| r["streams"][0]["width"] = json!(640)),
            ("matrix", |r| {
                r["streams"][0]["color_space"] = json!("bt709")
            }),
            ("missing transfer", |r| {
                r["streams"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("color_transfer");
            }),
            ("short video", |r| {
                r["streams"][0]["duration"] = json!("1.900000")
            }),
            ("long audio", |r| {
                r["streams"][1]["duration"] = json!("2.100000")
            }),
            ("language", |r| {
                r["streams"][1]["tags"]["language"] = json!("ger")
            }),
            ("location", |r| {
                r["format"]["tags"]["location"] = json!("+37.5665+126.9780/")
            }),
        ];
        for (what, change) in changes {
            let mut r = report();
            change(&mut r);
            assert!(checked(&r).is_err(), "{what} passed");
        }
        let mut r = report();
        r["streams"][0]["tags"]["creation_time"] = json!("2026-01-01T00:00:00Z");
        assert!(matches!(checked(&r), Err(VerifyError::Metadata(k)) if k == "creation_time"));
    }

    /// The transfer the encoder is told as `bt470bg` ffprobe may also name `gamma28`.
    #[test]
    fn colour_names_are_compared_as_values() {
        let mut r = report();
        r["streams"][0]["color_transfer"] = json!("gamma28");
        checked(&r).unwrap();
    }

    #[test]
    fn audio_that_ends_early_passes() {
        let mut r = report();
        r["streams"][1]["duration"] = json!("0.500000");
        checked(&r).unwrap();
    }

    /// Sound in the source up to the video's end must reach it in the output, from wherever it
    /// starts.
    #[test]
    fn audio_must_last_as_long_as_the_sources() {
        let mut to_the_end = expected();
        to_the_end.audio[0].to_the_end = true;
        let check = |r: &serde_json::Value| check_report(r.to_string().as_bytes(), &to_the_end);
        let mut r = report();
        check(&r).unwrap();
        r["streams"][1]["duration"] = json!("1.500000");
        assert!(matches!(check(&r), Err(VerifyError::Duration { .. })));
        r["streams"][1]["start_time"] = json!("0.500000");
        check(&r).unwrap();
    }
}
