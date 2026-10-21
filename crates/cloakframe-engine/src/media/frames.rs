//! The processed stream's frame timestamps from ffprobe's packet list, read as it streams in.
//! Listing packets reads the file but decodes nothing, except the first group of pictures when the
//! decoder may drop some of it.

use std::collections::HashSet;
use std::fmt;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::Stdio;

use cloakframe_core::timeline::{SourceFrames, SourceFramesError, TimelineSpec};

use crate::cancel::CancelToken;
use crate::media::probe::MAX_FRAMES;
use crate::media::process::{ToolCommand, ToolError};

#[derive(Debug)]
pub enum FramesError {
    Tool(ToolError),
    /// A line of the packet list this reader does not understand.
    Malformed(String),
    /// A frame of the processed stream has no presentation timestamp.
    NoTimestamp,
    TooMany,
    Frames(SourceFramesError),
}

impl fmt::Display for FramesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tool(e) => write!(f, "cannot list the video frames: {e}"),
            Self::Malformed(line) => write!(f, "cannot read ffprobe's packet list: {line}"),
            Self::NoTimestamp => f.write_str("a video frame has no timestamp"),
            Self::TooMany => f.write_str("the video frame count exceeds the limit"),
            Self::Frames(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for FramesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Tool(e) => Some(e),
            Self::Frames(e) => Some(e),
            _ => None,
        }
    }
}

pub fn list(
    ffprobe: &Path,
    input: &Path,
    spec: &TimelineSpec,
    cancel: &CancelToken,
) -> Result<SourceFrames, FramesError> {
    let mut command = ToolCommand::ffprobe(ffprobe);
    command
        .args([
            "-show_entries",
            "packet=stream_index,pts,duration,flags",
            "-print_format",
            "compact=print_section=0",
        ])
        .input_file(input);
    let mut running = command
        .spawn(Stdio::null(), Stdio::piped(), cancel)
        .map_err(FramesError::Tool)?;
    let lines = BufReader::new(running.take_stdout().expect("stdout is piped")).lines();
    let read = collect(
        lines.map(|l| l.map_err(|e| FramesError::Tool(ToolError::Io(e)))),
        spec,
    );
    // A read that stopped early leaves ffprobe writing; ending it first lets wait() return.
    if read.is_err() {
        running.kill();
    }
    let waited = running.wait().map_err(FramesError::Tool);
    // The read's error comes first: after it, the wait reports the kill above.
    let (mut kept, last_duration) = match (read, waited) {
        (Err(e), _) | (Ok(_), Err(e)) => return Err(e),
        (Ok(read), Ok(())) => read,
    };
    if let Some(end) = unclean_start(&kept) {
        let decoded = decoded_timestamps(ffprobe, input, spec.stream, end, cancel)?;
        kept.retain(|p| p.place >= end || decoded.contains(&p.pts));
    }
    let pts = kept.into_iter().map(|p| p.pts).collect();
    SourceFrames::new(spec.time_base, pts, last_duration).map_err(FramesError::Frames)
}

/// A packet of the processed stream that the container keeps, in decoding order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Kept {
    pts: i64,
    key: bool,
    /// Its place among all of the stream's packets, the discarded ones too.
    place: u64,
}

/// The packets of the processed stream, without the ones the container marks discarded
/// (`D`, e.g. before an edit list's start), with the duration of the last one in presentation
/// order.
fn collect(
    lines: impl Iterator<Item = Result<String, FramesError>>,
    spec: &TimelineSpec,
) -> Result<(Vec<Kept>, Option<i64>), FramesError> {
    let mut kept = Vec::new();
    let mut place = 0;
    let mut last: Option<(i64, Option<i64>)> = None;
    for line in lines {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let packet = Packet::parse(&line).ok_or_else(|| FramesError::Malformed(line.clone()))?;
        if packet.stream != spec.stream {
            continue;
        }
        place += 1;
        if packet.discarded {
            continue;
        }
        let pts = packet.pts.ok_or(FramesError::NoTimestamp)?;
        if kept.len() as u64 >= MAX_FRAMES {
            return Err(FramesError::TooMany);
        }
        kept.push(Kept {
            pts,
            key: packet.key,
            place: place - 1,
        });
        if last.is_none_or(|(t, _)| pts > t) {
            last = Some((pts, packet.duration));
        }
    }
    Ok((kept, last.and_then(|(_, d)| d)))
}

/// Where the first group of pictures ends, as a count of the stream's packets (`u64::MAX` when
/// no second keyframe follows), if the decoder may drop some of it: the stream does not open on
/// a keyframe, or packets after the first keyframe come before it in presentation order. Those
/// are an open GOP's leading pictures, which refer to a GOP the file no longer has once it was
/// cut there without an edit list to hide them.
fn unclean_start(kept: &[Kept]) -> Option<u64> {
    let first_key = kept.iter().position(|p| p.key)?;
    let end = kept[first_key + 1..]
        .iter()
        .find(|p| p.key)
        .map_or(u64::MAX, |p| p.place);
    let leading = kept[first_key + 1..]
        .iter()
        .take_while(|p| p.place < end)
        .any(|p| p.pts < kept[first_key].pts);
    (first_key > 0 || leading).then_some(end)
}

/// The presentation timestamps of the frames the decoder gives for the stream's first `packets`
/// packets (all of them for `u64::MAX`).
fn decoded_timestamps(
    ffprobe: &Path,
    input: &Path,
    stream: u32,
    packets: u64,
    cancel: &CancelToken,
) -> Result<HashSet<i64>, FramesError> {
    let mut command = ToolCommand::ffprobe(ffprobe);
    command.args(["-select_streams", &stream.to_string()]);
    if packets != u64::MAX {
        command.args(["-read_intervals", &format!("%+#{packets}")]);
    }
    command
        .args([
            "-show_entries",
            "frame=best_effort_timestamp",
            "-print_format",
            "compact=print_section=0",
        ])
        .input_file(input);
    let out = command.output(cancel).map_err(FramesError::Tool)?;
    let mut decoded = HashSet::new();
    for line in String::from_utf8_lossy(&out).lines() {
        if line.is_empty() {
            continue;
        }
        let malformed = || FramesError::Malformed(line.to_owned());
        let (key, value) = line
            .split('|')
            .find(|f| !f.is_empty())
            .and_then(|f| f.split_once('='))
            .ok_or_else(malformed)?;
        if key != "best_effort_timestamp" {
            return Err(malformed());
        }
        if value != "N/A" {
            decoded.insert(value.parse().map_err(|_| malformed())?);
        }
    }
    Ok(decoded)
}

struct Packet {
    stream: u32,
    pts: Option<i64>,
    duration: Option<i64>,
    key: bool,
    discarded: bool,
}

impl Packet {
    /// `stream_index=0|pts=37500|duration=1001|flags=K__`; `N/A` is a missing value. A packet
    /// with side data ends in an empty field.
    fn parse(line: &str) -> Option<Self> {
        let (mut stream, mut pts, mut duration, mut flags) = (None, None, None, None);
        for field in line.split('|').filter(|f| !f.is_empty()) {
            let (key, value) = field.split_once('=')?;
            let value = (value != "N/A").then_some(value);
            match key {
                "stream_index" => stream = Some(value?.parse().ok()?),
                "pts" => pts = value.map(str::parse).transpose().ok()?,
                "duration" => duration = value.map(str::parse).transpose().ok()?,
                "flags" => flags = value,
                _ => {}
            }
        }
        let flags = flags?;
        Some(Self {
            stream: stream?,
            pts,
            duration,
            key: flags.contains('K'),
            discarded: flags.contains('D'),
        })
    }
}

#[cfg(test)]
mod tests {
    use cloakframe_core::timeline::{ColorTags, Rational, Rotation, Seconds, VideoCodec};

    use super::*;

    fn spec(stream: u32) -> TimelineSpec {
        TimelineSpec {
            stream,
            codec: VideoCodec::H264,
            coded_width: 64,
            coded_height: 48,
            rotation: Rotation::None,
            sample_aspect: Rational::new(1, 1).unwrap(),
            rate: Rational::new(30, 1).unwrap(),
            nominal_rate: Rational::new(30, 1),
            average_rate: Rational::new(30, 1),
            variable_rate: false,
            start: Seconds::ZERO,
            time_base: Rational::new(1, 15360).unwrap(),
            container_start_seconds: 0.0,
            duration_seconds: Some(1.0),
            estimated_frames: 30,
            color: ColorTags::default().complete(64, 48),
            audio: Vec::new(),
            other_video_streams: 0,
        }
    }

    fn read(lines: &[&str], stream: u32) -> Result<(Vec<i64>, Option<i64>), FramesError> {
        collect(lines.iter().map(|l| Ok((*l).to_owned())), &spec(stream))
            .map(|(kept, last)| (kept.into_iter().map(|p| p.pts).collect(), last))
    }

    fn packets(list: &[(i64, bool)]) -> Vec<Kept> {
        list.iter()
            .enumerate()
            .map(|(place, &(pts, key))| Kept {
                pts,
                key,
                place: place as u64,
            })
            .collect()
    }

    #[test]
    fn a_start_with_leading_pictures_or_without_a_keyframe_is_unclean() {
        // Closed GOP: the B-frames after the keyframe come after it on screen too.
        let closed = packets(&[(0, true), (3, false), (1, false), (2, false), (4, true)]);
        assert_eq!(unclean_start(&closed), None);
        // Open GOP cut at a recovery point: pictures shown before the keyframe follow it.
        let open = packets(&[(3, true), (1, false), (2, false), (6, false), (7, true)]);
        assert_eq!(unclean_start(&open), Some(4));
        let late_key = packets(&[(0, false), (1, true), (2, false)]);
        assert_eq!(unclean_start(&late_key), Some(u64::MAX));
        assert_eq!(unclean_start(&packets(&[(0, false), (1, false)])), None);
        assert_eq!(unclean_start(&[]), None);
    }

    #[test]
    fn only_the_processed_streams_kept_packets_count() {
        let (pts, last) = read(
            &[
                "stream_index=0|pts=1024|duration=512|flags=K__",
                "stream_index=1|pts=-1024|duration=1024|flags=KD_|",
                "stream_index=0|pts=0|duration=512|flags=__D",
                "stream_index=0|pts=2048|duration=500|flags=___",
                "stream_index=0|pts=1536|duration=512|flags=___",
                "",
            ],
            0,
        )
        .unwrap();
        assert_eq!(pts, [1024, 2048, 1536]);
        assert_eq!(last, Some(500));
    }

    #[test]
    fn a_frame_without_a_timestamp_or_an_odd_line_is_refused() {
        assert!(matches!(
            read(&["stream_index=0|pts=N/A|duration=512|flags=K__"], 0),
            Err(FramesError::NoTimestamp)
        ));
        assert!(matches!(
            read(&["stream_index=1|pts=N/A|duration=512|flags=K__"], 0),
            Ok((pts, None)) if pts.is_empty()
        ));
        for odd in [
            "pts=0|flags=K__",
            "stream_index=0|pts=x|flags=K__",
            "garbage",
            "stream_index=0|pts=0",
        ] {
            assert!(
                matches!(read(&[odd], 0), Err(FramesError::Malformed(_))),
                "{odd}"
            );
        }
    }
}
