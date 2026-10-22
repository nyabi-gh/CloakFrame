//! Output frames into FFmpeg as raw RGB and out as MP4, encoded with the source's colour
//! description (D1), padded to an even size (P-5) and with the source's audio cut to the video's
//! interval.

use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::process::{ChildStdin, Stdio};

use cloakframe_core::timeline::{AudioStream, Range, TimelineSpec};

use crate::cancel::CancelToken;
use crate::detect::Rgb8Image;
use crate::media::decode::swscale_matrix;
use crate::media::process::{Supervised, ToolCommand, ToolError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputCodec {
    H264,
    Hevc,
}

/// 1.12.1's three settings and their x264 CRF; each encoder maps it as 1.12.1 did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    High,
    Balanced,
    SpaceSaver,
}

impl Quality {
    fn crf(self) -> u32 {
        match self {
            Self::High => 18,
            Self::Balanced => 21,
            Self::SpaceSaver => 24,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Encoder {
    pub name: &'static str,
    pub hardware: bool,
}

impl Encoder {
    pub fn software(codec: OutputCodec) -> Self {
        let name = match codec {
            OutputCodec::H264 => "libx264",
            OutputCodec::Hevc => "libx265",
        };
        Self {
            name,
            hardware: false,
        }
    }

    /// 1.12.1's candidates, in its order. AMF and VAAPI wait for a device to check them on.
    fn hardware_candidates(codec: OutputCodec) -> &'static [&'static str] {
        match codec {
            OutputCodec::H264 if cfg!(target_os = "macos") => &["h264_videotoolbox"],
            OutputCodec::Hevc if cfg!(target_os = "macos") => &["hevc_videotoolbox"],
            OutputCodec::H264 => &["h264_nvenc", "h264_qsv"],
            OutputCodec::Hevc => &["hevc_nvenc", "hevc_qsv"],
        }
    }

    fn args(self, quality: Quality) -> Vec<String> {
        let crf = quality.crf();
        let codec_crf = if self.name.starts_with("hevc_") || self.name == "libx265" {
            crf + 5
        } else {
            crf
        };
        let mut args = vec!["-c:v".to_owned(), self.name.to_owned()];
        let rest: Vec<String> = if self.name.ends_with("_nvenc") {
            [
                "-preset",
                "p6",
                "-rc",
                "vbr",
                "-cq",
                &codec_crf.to_string(),
                "-b:v",
                "0",
            ]
            .map(str::to_owned)
            .into()
        } else if self.name.ends_with("_qsv") {
            ["-preset", "slow", "-global_quality", &codec_crf.to_string()]
                .map(str::to_owned)
                .into()
        } else if self.name.ends_with("_videotoolbox") {
            let q = 100u32.saturating_sub(2 * crf).clamp(1, 100);
            ["-q:v".to_owned(), q.to_string()].into()
        } else {
            ["-preset", "medium", "-crf", &codec_crf.to_string()]
                .map(str::to_owned)
                .into()
        };
        args.extend(rest);
        args
    }
}

/// The encoders of one run. A hardware encoder that fails during an encode leaves the rest of
/// the run to software, as a GPU detector does (M3).
pub struct Encoders {
    hardware: bool,
    hardware_failed: bool,
}

impl Encoders {
    pub fn new(use_hardware: bool) -> Self {
        Self {
            hardware: use_hardware,
            hardware_failed: false,
        }
    }

    /// The first hardware candidate that encodes three frames of this item's size, rate and
    /// colour, or software.
    pub fn pick(
        &self,
        ffmpeg: &Path,
        codec: OutputCodec,
        spec: &TimelineSpec,
        cancel: &CancelToken,
    ) -> Result<Encoder, EncodeError> {
        if self.hardware && !self.hardware_failed {
            for &name in Encoder::hardware_candidates(codec) {
                let encoder = Encoder {
                    name,
                    hardware: true,
                };
                match trial_encode(ffmpeg, encoder, spec, cancel) {
                    Ok(()) => return Ok(encoder),
                    Err(EncodeError::Cancelled) => return Err(EncodeError::Cancelled),
                    Err(_) => {}
                }
            }
        }
        Ok(Encoder::software(codec))
    }

    pub fn note_hardware_failure(&mut self) {
        self.hardware_failed = true;
    }

    pub fn hardware_failed(&self) -> bool {
        self.hardware_failed
    }
}

const TRIAL_FRAMES: u64 = 3;

fn trial_encode(
    ffmpeg: &Path,
    encoder: Encoder,
    spec: &TimelineSpec,
    cancel: &CancelToken,
) -> Result<(), EncodeError> {
    let mut writer = FrameWriter::start(
        ffmpeg,
        encoder,
        Quality::Balanced,
        spec,
        TRIAL_FRAMES,
        None,
        Destination::Discard,
        cancel,
    )?;
    let (width, height) = spec.display_size();
    let black = Rgb8Image {
        width,
        height,
        data: vec![0; width as usize * height as usize * 3],
    };
    for _ in 0..TRIAL_FRAMES {
        writer.write(&black)?;
    }
    writer.finish()
}

#[derive(Debug)]
pub enum EncodeError {
    Tool(ToolError),
    /// The output file could not be made, or already exists.
    Output(io::Error),
    /// A frame of another size than the video's.
    FrameSize {
        expected: (u32, u32),
        got: (u32, u32),
    },
    /// The writer was given a different number of frames than it was started for.
    FrameCount {
        expected: u64,
        written: u64,
    },
    Cancelled,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tool(e) => write!(f, "cannot encode the video: {e}"),
            Self::Output(e) => write!(f, "cannot create the video file: {e}"),
            Self::FrameSize { expected, got } => write!(
                f,
                "a {}x{} frame was given for a {}x{} video",
                got.0, got.1, expected.0, expected.1
            ),
            Self::FrameCount { expected, written } => write!(
                f,
                "the video should have {expected} frames but {written} were written"
            ),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for EncodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Tool(e) => Some(e),
            Self::Output(e) => Some(e),
            _ => None,
        }
    }
}

fn tool_error(e: ToolError) -> EncodeError {
    match e {
        ToolError::Cancelled => EncodeError::Cancelled,
        e => EncodeError::Tool(e),
    }
}

/// The encoded size: odd sides grow by one, because 4:2:0 cannot code or crop to an odd size.
pub fn encoded_size((width, height): (u32, u32)) -> (u32, u32) {
    (width + width % 2, height + height % 2)
}

/// Enough for one packet of AAC at 8 kHz (128 ms) and of MP3 or AC-3 at any rate.
const AUDIO_PREROLL_SECONDS: f64 = 0.25;

/// Codecs MP4 carries that 1.12.1 copied; the rest become AAC at 192 kb/s.
const COPIED_AUDIO: [&str; 5] = ["aac", "mp3", "ac3", "eac3", "alac"];

#[derive(Clone, Copy)]
pub enum Destination<'a> {
    /// A new file, made here; an existing one is never replaced.
    File(&'a Path),
    /// A file this process made for the output, such as a [`TempOutput`]'s, which FFmpeg writes
    /// over.
    ///
    /// [`TempOutput`]: crate::publish::TempOutput
    Made(&'a Path),
    /// Encoded and thrown away, to see whether an encoder works.
    Discard,
}

/// Frames in display orientation, at the source's display size.
pub struct FrameWriter {
    running: Option<Supervised>,
    stdin: Option<ChildStdin>,
    size: (u32, u32),
    padded: Vec<u8>,
    expected: u64,
    written: u64,
}

impl FrameWriter {
    /// `audio_from` is the source whose audio streams in `spec` are kept.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        ffmpeg: &Path,
        encoder: Encoder,
        quality: Quality,
        spec: &TimelineSpec,
        frames: u64,
        audio_from: Option<&Path>,
        destination: Destination<'_>,
        cancel: &CancelToken,
    ) -> Result<Self, EncodeError> {
        let size = spec.display_size();
        let (width, height) = encoded_size(size);
        let rate = spec.rate;
        let mut command = ToolCommand::ffmpeg(ffmpeg);
        command
            .args(["-f", "rawvideo", "-pix_fmt", "rgb24"])
            .args(["-s", &format!("{width}x{height}")])
            .args(["-framerate", &format!("{}/{}", rate.num(), rate.den())])
            .input_pipe();
        let length = frames as f64 * f64::from(rate.den()) / f64::from(rate.num());
        let audio: &[AudioStream] = match audio_from {
            Some(_) => &spec.audio,
            None => &[],
        };
        if let (Some(source), false) = (audio_from, audio.is_empty()) {
            // Moved so the video's start is zero, not cut with -ss: FFmpeg drops the copied
            // packets before a seek point, and AAC, MP3 and AC-3 decode a packet only after the
            // one before it, so a cut on a packet boundary lost the first frame of sound.
            command
                .args([
                    "-copyts",
                    "-itsoffset",
                    &format!("-{:.9}", spec.start.as_f64()),
                ])
                .input_file(source);
        }
        command
            .args(["-map", "0:v"])
            .args(encoder.args(quality))
            .args(["-vf", &output_filter(spec)]);
        if encoder.name.starts_with("hevc_") || encoder.name == "libx265" {
            command.args(["-tag:v", "hvc1"]);
        }
        for (i, stream) in audio.iter().enumerate() {
            command.args(["-map", &format!("1:{}", stream.index)]);
            if COPIED_AUDIO.contains(&stream.codec.as_str()) {
                command.args([format!("-c:a:{i}").as_str(), "copy"]);
            } else {
                command.args([format!("-c:a:{i}").as_str(), "aac"]);
                command.args([format!("-b:a:{i}").as_str(), "192k"]);
            }
            if let Some(language) = &stream.language {
                command.args([format!("-metadata:s:a:{i}"), format!("language={language}")]);
            }
        }
        if !audio.is_empty() {
            // The packets just before the start stay for the decoder, hidden by the edit list
            // FFmpeg writes; the rest of the audio before the video is dropped. -t ends the cut
            // at the packet covering the end, which keeps its full length, up to one AAC frame
            // past the video, unless its duration is shortened.
            let end = format!("{length:.9}/TB");
            command.args([
                "-t".to_owned(),
                format!("{length:.9}"),
                "-bsf:a".to_owned(),
                format!(
                    "noise=drop='lt(pts*tb\\,-{AUDIO_PREROLL_SECONDS})',\
                     setts=duration='if(gt(PTS+DURATION,{end}),{end}-PTS,DURATION)'"
                ),
            ]);
        }
        command.args(["-map_metadata", "-1", "-map_chapters", "-1"]);
        match destination {
            Destination::File(path) | Destination::Made(path) => {
                // FFmpeg 9 refuses an existing output yet exits with 0, so the file is made
                // beforehand, never replacing one, and FFmpeg writes into it.
                if let Destination::File(path) = destination {
                    File::create_new(path).map_err(EncodeError::Output)?;
                }
                command
                    .args(["-y", "-movflags", "+faststart", "-f", "mp4"])
                    .output_file(path);
            }
            Destination::Discard => {
                command.args(["-f", "null", "-"]);
            }
        }
        // stdout is never read, so it must not be a pipe the tool could fill and block on.
        let mut running = command
            .spawn(Stdio::piped(), Stdio::null(), cancel)
            .map_err(tool_error)?;
        let stdin = running.take_stdin().expect("stdin is piped");
        Ok(Self {
            running: Some(running),
            stdin: Some(stdin),
            size,
            padded: Vec::new(),
            expected: frames,
            written: 0,
        })
    }

    pub fn write(&mut self, frame: &Rgb8Image) -> Result<(), EncodeError> {
        if (frame.width, frame.height) != self.size {
            return Err(self.end_with(EncodeError::FrameSize {
                expected: self.size,
                got: (frame.width, frame.height),
            }));
        }
        if self.written == self.expected {
            return Err(self.end_with(EncodeError::FrameCount {
                expected: self.expected,
                written: self.written + 1,
            }));
        }
        let bytes = if encoded_size(self.size) == self.size {
            &frame.data
        } else {
            pad_edges(frame, &mut self.padded);
            &self.padded
        };
        let stdin = self.stdin.as_mut().expect("not finished");
        match stdin.write_all(bytes) {
            Ok(()) => {
                self.written += 1;
                Ok(())
            }
            Err(e) => Err(self.failed(e)),
        }
    }

    /// Ends the input and waits for FFmpeg to finish the file.
    pub fn finish(mut self) -> Result<(), EncodeError> {
        if self.written != self.expected {
            return Err(self.end_with(EncodeError::FrameCount {
                expected: self.expected,
                written: self.written,
            }));
        }
        self.stdin = None;
        let running = self.running.take().expect("not finished");
        running.wait().map_err(tool_error)
    }

    /// A write fails when FFmpeg has stopped reading, usually because it failed; its own error
    /// says why.
    fn failed(&mut self, e: io::Error) -> EncodeError {
        self.stdin = None;
        let running = self.running.take().expect("not finished");
        match running.wait() {
            Err(ToolError::Cancelled) => EncodeError::Cancelled,
            Err(tool) => EncodeError::Tool(tool),
            Ok(()) => EncodeError::Tool(ToolError::Io(e)),
        }
    }

    fn end_with(&mut self, error: EncodeError) -> EncodeError {
        self.stdin = None;
        if let Some(running) = self.running.take() {
            running.kill();
        }
        error
    }
}

/// The frame with its last column and row repeated to reach the encoded size.
fn pad_edges(frame: &Rgb8Image, out: &mut Vec<u8>) {
    let (width, height) = (frame.width as usize, frame.height as usize);
    let (out_width, out_height) = encoded_size((frame.width, frame.height));
    let (out_width, out_height) = (out_width as usize, out_height as usize);
    out.clear();
    out.reserve(out_width * out_height * 3);
    for y in 0..out_height {
        let row = &frame.data[y.min(height - 1) * width * 3..][..width * 3];
        out.extend_from_slice(row);
        if out_width > width {
            out.extend_from_slice(&row[(width - 1) * 3..]);
        }
    }
}

/// RGB to 4:2:0 with the source's matrix and range, tagged with its full colour description.
/// The tags go on the frames: the encoders take them from there and ignore `-color_primaries`
/// and `-color_trc` for frames that carry none.
fn output_filter(spec: &TimelineSpec) -> String {
    let sar = spec.sample_aspect;
    let (sar_num, sar_den) = if spec.rotation.swaps_sides() {
        (sar.den(), sar.num())
    } else {
        (sar.num(), sar.den())
    };
    let matrix = spec.output_matrix();
    let range = match spec.color.range {
        Range::Tv => "tv",
        Range::Pc => "pc",
    };
    format!(
        "setsar={sar_num}/{sar_den},\
         scale=out_color_matrix={}:out_range={range}:flags=accurate_rnd+full_chroma_int+bitexact,\
         format=yuv420p,\
         setparams=colorspace={}:color_primaries={}:color_trc={}:range={range}",
        swscale_matrix(matrix),
        matrix.ffmpeg_name(),
        spec.color.primaries.ffmpeg_name(),
        spec.color.transfer.ffmpeg_name(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn odd_sides_repeat_their_last_column_and_row() {
        assert_eq!(encoded_size((641, 361)), (642, 362));
        assert_eq!(encoded_size((640, 360)), (640, 360));
        let frame = Rgb8Image {
            width: 3,
            height: 1,
            data: vec![1, 2, 3, 4, 5, 6, 7, 8, 9],
        };
        let mut out = Vec::new();
        pad_edges(&frame, &mut out);
        assert_eq!(
            out,
            [
                1, 2, 3, 4, 5, 6, 7, 8, 9, 7, 8, 9, //
                1, 2, 3, 4, 5, 6, 7, 8, 9, 7, 8, 9,
            ]
        );
    }

    #[test]
    fn quality_maps_to_each_encoder_as_in_1_12_1() {
        let args = |name, hardware, quality| Encoder { name, hardware }.args(quality).join(" ");
        assert_eq!(
            args("libx264", false, Quality::High),
            "-c:v libx264 -preset medium -crf 18"
        );
        assert_eq!(
            args("libx265", false, Quality::Balanced),
            "-c:v libx265 -preset medium -crf 26"
        );
        assert_eq!(
            args("hevc_nvenc", true, Quality::High),
            "-c:v hevc_nvenc -preset p6 -rc vbr -cq 23 -b:v 0"
        );
        assert_eq!(
            args("h264_qsv", true, Quality::SpaceSaver),
            "-c:v h264_qsv -preset slow -global_quality 24"
        );
        assert_eq!(
            args("h264_videotoolbox", true, Quality::High),
            "-c:v h264_videotoolbox -q:v 64"
        );
    }

    #[test]
    fn a_failed_hardware_encoder_leaves_the_run_to_software() {
        let mut encoders = Encoders::new(true);
        encoders.note_hardware_failure();
        assert!(encoders.hardware_failed());
        // No candidate is tried, so no FFmpeg is needed.
        let ffmpeg = Path::new("/nonexistent/ffmpeg");
        let picked = encoders
            .pick(ffmpeg, OutputCodec::Hevc, &spec(), &CancelToken::new())
            .unwrap();
        assert_eq!(picked, Encoder::software(OutputCodec::Hevc));
    }

    fn spec() -> TimelineSpec {
        use cloakframe_core::timeline::{ColorTags, Rational, Rotation, Seconds, VideoCodec};
        let rate = Rational::new(30, 1).unwrap();
        TimelineSpec {
            stream: 0,
            codec: VideoCodec::H264,
            coded_width: 64,
            coded_height: 48,
            rotation: Rotation::Quarter,
            sample_aspect: Rational::new(4, 3).unwrap(),
            rate,
            nominal_rate: Some(rate),
            average_rate: Some(rate),
            variable_rate: false,
            start: Seconds::ZERO,
            time_base: Rational::new(1, 1000).unwrap(),
            container_start_seconds: 0.0,
            duration_seconds: Some(1.0),
            estimated_frames: 30,
            color: ColorTags::default().complete(64, 48),
            audio: Vec::new(),
            other_video_streams: 0,
        }
    }

    #[test]
    fn a_turned_frame_takes_the_inverse_aspect() {
        assert!(output_filter(&spec()).starts_with("setsar=3/4,"));
    }
}
