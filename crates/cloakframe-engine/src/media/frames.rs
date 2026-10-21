//! The processed stream's frame timestamps from ffprobe's packet list, read as it streams in.
//! Listing packets reads the file but decodes nothing.

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
    let (pts, last_duration) = match (read, waited) {
        (Err(e), _) | (Ok(_), Err(e)) => return Err(e),
        (Ok(read), Ok(())) => read,
    };
    SourceFrames::new(spec.time_base, pts, last_duration).map_err(FramesError::Frames)
}

/// The packets of the processed stream, without the ones the container marks discarded
/// (`D`, e.g. before an edit list's start), with the duration of the last one in presentation
/// order.
fn collect(
    lines: impl Iterator<Item = Result<String, FramesError>>,
    spec: &TimelineSpec,
) -> Result<(Vec<i64>, Option<i64>), FramesError> {
    let mut pts = Vec::new();
    let mut last: Option<(i64, Option<i64>)> = None;
    for line in lines {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let packet = Packet::parse(&line).ok_or_else(|| FramesError::Malformed(line.clone()))?;
        if packet.stream != spec.stream || packet.discarded {
            continue;
        }
        let time = packet.pts.ok_or(FramesError::NoTimestamp)?;
        if pts.len() as u64 >= MAX_FRAMES {
            return Err(FramesError::TooMany);
        }
        pts.push(time);
        if last.is_none_or(|(t, _)| time > t) {
            last = Some((time, packet.duration));
        }
    }
    Ok((pts, last.and_then(|(_, d)| d)))
}

struct Packet {
    stream: u32,
    pts: Option<i64>,
    duration: Option<i64>,
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
        Some(Self {
            stream: stream?,
            pts,
            duration,
            discarded: flags?.contains('D'),
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
            variable_rate: false,
            start: Seconds::ZERO,
            time_base: Rational::new(1, 15360).unwrap(),
            container_start_seconds: 0.0,
            duration_seconds: 1.0,
            estimated_frames: 30,
            color: ColorTags::default().complete(48),
            audio: Vec::new(),
            other_video_streams: 0,
        }
    }

    fn read(lines: &[&str], stream: u32) -> Result<(Vec<i64>, Option<i64>), FramesError> {
        collect(lines.iter().map(|l| Ok((*l).to_owned())), &spec(stream))
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
