//! A video ready for its passes: ffprobe's report and packet list, read once.

use std::fmt;
use std::path::Path;

use cloakframe_core::timeline::{SourceFrames, TimelineSpec};

use crate::cancel::CancelToken;
use crate::media::frames::{self, FramesError};
use crate::media::probe::{self, ProbeError};

pub struct Source {
    pub spec: TimelineSpec,
    pub frames: SourceFrames,
}

#[derive(Debug)]
pub enum OpenError {
    Probe(ProbeError),
    Frames(FramesError),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Probe(e) => e.fmt(f),
            Self::Frames(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for OpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Probe(e) => Some(e),
            Self::Frames(e) => Some(e),
        }
    }
}

/// The timeline from ffprobe's report, with the constant rate its frame list calls for.
pub fn open(ffprobe: &Path, input: &Path, cancel: &CancelToken) -> Result<Source, OpenError> {
    let mut spec = probe::probe(ffprobe, input, cancel).map_err(OpenError::Probe)?;
    let frames = frames::list(ffprobe, input, &spec, cancel).map_err(OpenError::Frames)?;
    spec.rate = frames.constant_rate(spec.start, spec.nominal_rate, spec.average_rate);
    Ok(Source { spec, frames })
}
