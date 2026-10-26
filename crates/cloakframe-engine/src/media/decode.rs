//! Source frames out of FFmpeg as raw RGB, display-oriented, in the source's own colour space
//! decoded with the matrix and range of the [`TimelineSpec`] (D1). Passes read every source frame
//! unfiltered; [`SourceFrames`] decides which output frame shows which (P-3).

use std::fmt;
use std::io::{self, Read};
use std::path::Path;
use std::process::{ChildStdout, Stdio};

use cloakframe_core::timeline::{Matrix, Range, SourceFrames, TimelineSpec};

use crate::cancel::CancelToken;
use crate::detect::Rgb8Image;
use crate::media::process::{Supervised, ToolCommand, ToolError};
use crate::run::{Classify, FailureKind};

#[derive(Debug)]
pub enum DecodeError {
    Tool(ToolError),
    /// FFmpeg gave a different number of frames than the packet list has.
    FrameCount {
        listed: usize,
        decoded: usize,
    },
    /// The frame asked for was not among those decoded after the seek.
    NoFrame,
    Cancelled,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tool(e) => write!(f, "cannot decode the video: {e}"),
            Self::FrameCount { listed, decoded } => write!(
                f,
                "the video has {listed} frames but {decoded} could be decoded"
            ),
            Self::NoFrame => f.write_str("the requested video frame could not be decoded"),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl Classify for DecodeError {
    fn kind(&self) -> FailureKind {
        match self {
            Self::Tool(e) => e.kind_or(FailureKind::Unreadable),
            Self::FrameCount { .. } | Self::NoFrame => FailureKind::Unreadable,
            Self::Cancelled => FailureKind::Other,
        }
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Tool(e) => Some(e),
            _ => None,
        }
    }
}

fn tool_error(e: ToolError) -> DecodeError {
    match e {
        ToolError::Cancelled => DecodeError::Cancelled,
        e => DecodeError::Tool(e),
    }
}

/// The display size scaled so its long edge is at most `long_edge`, each side at least 2
/// (1.12.1's rule).
pub fn frame_size(spec: &TimelineSpec, long_edge: Option<u32>) -> (u32, u32) {
    let (width, height) = spec.display_size();
    let longest = width.max(height);
    match long_edge {
        Some(limit) if limit < longest => {
            let scale = f64::from(limit) / f64::from(longest);
            let side = |d: u32| ((f64::from(d) * scale).round() as u32).max(2);
            (side(width), side(height))
        }
        _ => (width, height),
    }
}

/// swscale's names for the matrix; `bt470` is BT.470 BG's, which SMPTE 170M shares.
pub(super) fn swscale_matrix(matrix: Matrix) -> &'static str {
    match matrix {
        Matrix::Bt709 => "bt709",
        Matrix::Bt470bg => "bt470",
        Matrix::Smpte170m => "smpte170m",
        Matrix::Smpte240m => "smpte240m",
        Matrix::Fcc => "fcc",
        Matrix::Bt2020nc => "bt2020",
        // RGB-coded frames reach the scaler as planar RGB, which takes no matrix.
        Matrix::Gbr => "bt709",
    }
}

/// To full-range RGB with the spec's matrix and range rather than the frame's own tags, which
/// may be missing. `bitexact` and the accurate paths make the pixels the same on every CPU.
fn rgb_filter(spec: &TimelineSpec, (width, height): (u32, u32)) -> String {
    let range = match spec.color.range {
        Range::Tv => "tv",
        Range::Pc => "pc",
    };
    format!(
        "scale=w={width}:h={height}:in_color_matrix={}:in_range={range}:out_range=pc:\
         flags=area+accurate_rnd+full_chroma_int+bitexact,format=rgb24",
        swscale_matrix(spec.color.matrix)
    )
}

fn stream_map(spec: &TimelineSpec) -> String {
    format!("0:{}", spec.stream)
}

/// Every source frame of the processed stream, in presentation order.
pub struct FrameReader {
    running: Option<Supervised>,
    stdout: Option<ChildStdout>,
    size: (u32, u32),
    listed: usize,
    decoded: usize,
    cancel: CancelToken,
}

impl FrameReader {
    pub fn start(
        ffmpeg: &Path,
        input: &Path,
        spec: &TimelineSpec,
        frames: &SourceFrames,
        size: (u32, u32),
        cancel: &CancelToken,
    ) -> Result<Self, DecodeError> {
        Self::start_at(ffmpeg, input, spec, frames, 0, size, cancel)
    }

    /// Source frames `from..`, for the preview's playback: a seek to frame `from` as
    /// [`source_frame`] seeks, then every frame from its timestamp on.
    pub fn start_at(
        ffmpeg: &Path,
        input: &Path,
        spec: &TimelineSpec,
        frames: &SourceFrames,
        from: usize,
        size: (u32, u32),
        cancel: &CancelToken,
    ) -> Result<Self, DecodeError> {
        if from >= frames.len() {
            return Err(DecodeError::NoFrame);
        }
        let mut command = ToolCommand::ffmpeg(ffmpeg);
        // -xerror: a damaged frame fails the item instead of silently shortening the video.
        command.arg("-xerror");
        let filter = if from == 0 {
            rgb_filter(spec, size)
        } else {
            command.args(["-copyts", "-ss", &seek_to(spec, frames, from)]);
            format!(
                "select=gte(pts\\,{}),{}",
                frames.pts(from),
                rgb_filter(spec, size)
            )
        };
        command
            .input_file(input)
            .args(["-map", &stream_map(spec), "-fps_mode", "passthrough"])
            .args(["-vf", &filter])
            .args(["-f", "rawvideo", "-pix_fmt", "rgb24"])
            .output_pipe();
        let mut running = command
            .spawn(Stdio::null(), Stdio::piped(), cancel)
            .map_err(tool_error)?;
        let stdout = running.take_stdout().expect("stdout is piped");
        Ok(Self {
            running: Some(running),
            stdout: Some(stdout),
            size,
            listed: frames.len() - from,
            decoded: 0,
            cancel: cancel.clone(),
        })
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The next frame into `frame`, which is resized as needed; `false` once FFmpeg is done
    /// and gave exactly the listed frames.
    pub fn next_into(&mut self, frame: &mut Rgb8Image) -> Result<bool, DecodeError> {
        let (width, height) = self.size;
        frame.width = width;
        frame.height = height;
        frame.data.resize(width as usize * height as usize * 3, 0);
        // The process watcher looks for a cancel only now and then; a short video can be read to
        // its end before it does, and would then pass as complete.
        if self.cancel.is_cancelled() {
            return Err(self.end_with(DecodeError::Cancelled));
        }
        let stdout = self.stdout.as_mut().expect("not finished");
        match read_frame(stdout, &mut frame.data) {
            Ok(true) => {
                self.decoded += 1;
                if self.decoded > self.listed {
                    return Err(self.end_with(DecodeError::FrameCount {
                        listed: self.listed,
                        decoded: self.decoded,
                    }));
                }
                Ok(true)
            }
            Ok(false) => {
                let running = self.running.take().expect("not finished");
                running.wait().map_err(tool_error)?;
                if self.decoded != self.listed {
                    return Err(DecodeError::FrameCount {
                        listed: self.listed,
                        decoded: self.decoded,
                    });
                }
                Ok(false)
            }
            Err(e) => {
                // Closing the pipe ends a tool still writing, without a kill that would make a
                // tool that had already failed look cancelled.
                self.stdout = None;
                let running = self.running.take().expect("not finished");
                Err(match running.wait() {
                    Err(ToolError::Cancelled) => DecodeError::Cancelled,
                    Err(tool) => DecodeError::Tool(tool),
                    Ok(()) => DecodeError::Tool(ToolError::Io(e)),
                })
            }
        }
    }

    fn end_with(&mut self, error: DecodeError) -> DecodeError {
        if let Some(running) = self.running.take() {
            running.kill();
        }
        error
    }
}

/// Fills `buf` with one frame; `false` at a clean end before the first byte of a frame.
fn read_frame(stdout: &mut ChildStdout, buf: &mut [u8]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < buf.len() {
        match stdout.read(&mut buf[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the video ended inside a frame",
                ));
            }
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

/// The `-ss` that lands at or before frame `k`. FFmpeg adds the container's start to it. The
/// margin covers the start time's decimal rounding; the frames before `k` are dropped by
/// matching its timestamp.
fn seek_to(spec: &TimelineSpec, frames: &SourceFrames, k: usize) -> String {
    let seek = (frames.time(k).as_f64() - spec.container_start_seconds - 0.001).max(0.0);
    format!("{seek:.6}")
}

/// Source frame `k` alone, for the preview: a seek to its time, then exactly the frame with its
/// timestamp, so it is the very frame a full pass gives as its `k`-th.
pub fn source_frame(
    ffmpeg: &Path,
    input: &Path,
    spec: &TimelineSpec,
    frames: &SourceFrames,
    k: usize,
    size: (u32, u32),
    cancel: &CancelToken,
) -> Result<Rgb8Image, DecodeError> {
    let mut command = ToolCommand::ffmpeg(ffmpeg);
    command
        .args(["-copyts", "-ss", &seek_to(spec, frames, k)])
        .input_file(input)
        .args(["-map", &stream_map(spec)])
        .args([
            "-vf",
            &format!(
                "select=eq(pts\\,{}),{}",
                frames.pts(k),
                rgb_filter(spec, size)
            ),
        ])
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24"])
        .output_pipe();
    let out = command.output(cancel).map_err(tool_error)?;
    let (width, height) = size;
    if out.len() != width as usize * height as usize * 3 {
        return Err(DecodeError::NoFrame);
    }
    Ok(Rgb8Image {
        width,
        height,
        data: out,
    })
}
