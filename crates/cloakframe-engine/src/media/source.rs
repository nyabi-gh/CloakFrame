//! A video ready for its passes: ffprobe's report and packet list, read once.

use std::fmt;
use std::path::Path;

use cloakframe_core::timeline::{SourceFrames, TimelineSpec};

use crate::cancel::CancelToken;
use crate::media::frames::{self, FramesError};
use crate::media::probe::{self, MAX_DURATION_SECONDS, MAX_FRAMES, ProbeError, Unsupported};

pub struct Source {
    pub spec: TimelineSpec,
    pub frames: SourceFrames,
}

#[derive(Debug)]
pub enum OpenError {
    Probe(ProbeError),
    Frames(FramesError),
    /// Past a limit the frame list shows and the report did not.
    Unsupported(Unsupported),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Probe(e) => e.fmt(f),
            Self::Frames(e) => e.fmt(f),
            Self::Unsupported(u) => u.fmt(f),
        }
    }
}

impl std::error::Error for OpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Probe(e) => Some(e),
            Self::Frames(e) => Some(e),
            Self::Unsupported(_) => None,
        }
    }
}

/// The timeline from ffprobe's report, with the constant rate its frame list calls for.
pub fn open(ffprobe: &Path, input: &Path, cancel: &CancelToken) -> Result<Source, OpenError> {
    let spec = probe::probe(ffprobe, input, cancel).map_err(OpenError::Probe)?;
    let frames = frames::list(ffprobe, input, &spec, cancel).map_err(OpenError::Frames)?;
    finish(spec, frames).map_err(OpenError::Unsupported)
}

/// Sets the output's rate and frame count from the frame list and checks the length and frame
/// limits again on it: the report's duration and frame count come from container headers, which
/// need not match the frames.
fn finish(mut spec: TimelineSpec, frames: SourceFrames) -> Result<Source, Unsupported> {
    spec.rate = frames.constant_rate(spec.start, spec.nominal_rate, spec.average_rate);
    let length = frames.end().as_f64() - spec.start.as_f64();
    if length > MAX_DURATION_SECONDS {
        return Err(Unsupported::Duration);
    }
    let output_frames = frames.output_frames(&spec);
    if output_frames > MAX_FRAMES {
        return Err(Unsupported::FrameCount);
    }
    spec.estimated_frames = output_frames;
    spec.duration_seconds.get_or_insert(length);
    Ok(Source { spec, frames })
}

#[cfg(test)]
mod tests {
    use cloakframe_core::timeline::{ColorTags, Rational, Rotation, Seconds, VideoCodec};

    use super::*;

    fn spec(duration_seconds: Option<f64>) -> TimelineSpec {
        let rate = Rational::new(30, 1);
        TimelineSpec {
            stream: 0,
            codec: VideoCodec::H264,
            coded_width: 64,
            coded_height: 48,
            rotation: Rotation::None,
            sample_aspect: Rational::new(1, 1).unwrap(),
            rate: rate.unwrap(),
            nominal_rate: rate,
            average_rate: rate,
            variable_rate: false,
            start: Seconds::ZERO,
            time_base: Rational::new(1, 1000).unwrap(),
            container_start_seconds: 0.0,
            duration_seconds,
            estimated_frames: 30,
            color: ColorTags::default().complete(64, 48),
            audio: Vec::new(),
            other_video_streams: 0,
        }
    }

    fn frames(pts: Vec<i64>) -> SourceFrames {
        SourceFrames::new(Rational::new(1, 1000).unwrap(), pts, Some(33)).unwrap()
    }

    #[test]
    fn the_frame_list_gives_the_duration_and_count_the_report_left_out() {
        let source = finish(spec(None), frames((0..60).map(|k| k * 100 / 3).collect())).unwrap();
        assert_eq!(source.spec.estimated_frames, 60);
        let length = source.spec.duration_seconds.unwrap();
        assert!((length - 2.0).abs() < 0.01, "{length}");
    }

    /// A header that says one second does not hide frames a day apart.
    #[test]
    fn limits_hold_on_the_frames_whatever_the_header_says() {
        let a_day_later = 86_401 * 1000;
        assert_eq!(
            finish(spec(Some(1.0)), frames(vec![0, 33, a_day_later])).err(),
            Some(Unsupported::Duration)
        );
        // Frames 4 ms apart show only at 240 fps; three and a half hours of it is 3,024,000.
        let later = 12_600 * 1000;
        assert_eq!(
            finish(spec(Some(1.0)), frames(vec![0, 4, 8, later])).err(),
            Some(Unsupported::FrameCount)
        );
    }
}
