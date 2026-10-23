//! M4's completion check (plan 8 M4 stage 5): videos exported end to end through the runner with
//! a corner a stand-in detector finds in every frame, tracked and masked as a face is (M5 stage
//! 2), from sources with each frame's number drawn in and clicks in their sound on
//! chosen frames, made here with the pinned FFmpeg. Every output frame `n` shows the source frame
//! an analysis pass reads as `shown_at(n)` (P-3 `analysis_equals_output`), every source frame
//! shows, and every click is heard as the first output frame showing its frame starts: the sound
//! within 1 ms of its place from the video's start, the frame within half an output frame of
//! its source time (the most the timeline rule moves a frame).
//!
//! Each source is exported twice: in H.264 with a hardware encoder where there is one (the
//! default), and in HEVC with libx265.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use cloakframe_core::detection::{Detection, ModelId, Thresholds};
use cloakframe_core::geometry::Rect;
use cloakframe_core::item::{ItemId, ItemState, PublishBasis, ReviewMode};
use cloakframe_core::timeline::{Rational, Rotation};
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::{DetectError, Detector, DetectorHandle, Found, Rgb8Image};
use cloakframe_engine::media::decode::{FrameReader, frame_size};
use cloakframe_engine::media::encode::OutputCodec;
use cloakframe_engine::media::process::ToolCommand;
use cloakframe_engine::media::source::{self, Source};
use cloakframe_engine::run::{self, Job, JobItem, Note, Start};
use cloakframe_engine::video::analysis::AnalysisSettings;
use cloakframe_engine::video::process::{Tools, VideoProcessor, VideoSettings};
use common::{TestDir, fixtures, frame_number, numbered_frame, required_env};

fn ffmpeg() -> PathBuf {
    required_env("CLOAKFRAME_FFMPEG")
}

fn ffprobe() -> PathBuf {
    required_env("CLOAKFRAME_FFPROBE")
}

fn opened(path: &Path) -> Source {
    source::open(&ffprobe(), path, &CancelToken::new()).unwrap()
}

/// Every frame a pass reads, at `long_edge` or the display size.
fn decoded(path: &Path, source: &Source, long_edge: Option<u32>) -> Vec<Rgb8Image> {
    let mut reader = FrameReader::start(
        &ffmpeg(),
        path,
        &source.spec,
        &source.frames,
        frame_size(&source.spec, long_edge),
        &CancelToken::new(),
    )
    .unwrap();
    let mut out = Vec::new();
    loop {
        let mut frame = Rgb8Image {
            width: 0,
            height: 0,
            data: Vec::new(),
        };
        if !reader.next_into(&mut frame).unwrap() {
            return out;
        }
        out.push(frame);
    }
}

fn numbers(frames: &[Rgb8Image], rotation: Rotation) -> Vec<u32> {
    frames
        .iter()
        .map(|f| frame_number(f.width, f.height, &f.data, rotation))
        .collect()
}

const SIZE: (u32, u32) = (320, 180);
const H264: &[&str] = &["-c:v", "libx264", "-bf", "3", "-pix_fmt", "yuv420p"];
/// A click on every seventh frame, from the third.
const CLICK_EVERY: usize = 7;
const CLICK_FROM: usize = 2;
const CLICK_SECONDS: f64 = 0.002;

/// A synthetic source: numbered frames, then a click at the start of every [`CLICK_EVERY`]-th
/// frame muxed in.
struct Recipe {
    name: &'static str,
    frames: u32,
    /// The raw frames' rate, before any retiming in `video`.
    rate: &'static str,
    /// The numbered video's filters and encoder.
    video: &'static [&'static str],
    container: &'static str,
    audio_codec: &'static str,
    audio_rate: u32,
    /// How much sooner the sound starts than the video; negative when later.
    audio_lead: f64,
    /// Input options for the video in the mux, such as `-display_rotation`.
    video_input: &'static [&'static str],
    /// Output options of the mux, such as `-output_ts_offset`.
    mux: &'static [&'static str],
    /// The constant rate the source is exported at.
    output_rate: (u32, u32),
}

impl Recipe {
    /// Five seconds of 30 fps H.264 with B-frames in MP4, AAC from the video's start.
    fn new(name: &'static str) -> Self {
        Self {
            name,
            frames: 150,
            rate: "30",
            video: H264,
            container: "mp4",
            audio_codec: "aac",
            audio_rate: 48_000,
            audio_lead: 0.0,
            video_input: &[],
            mux: &[],
            output_rate: (30, 1),
        }
    }

    /// The source, and when its clicks are from the video's start.
    fn make(&self, dir: &Path) -> (PathBuf, Vec<f64>) {
        let video = dir.join(format!("{}-video.{}", self.name, self.container));
        let mut c = ToolCommand::ffmpeg(&ffmpeg());
        c.args(["-f", "rawvideo", "-pix_fmt", "rgb24"])
            .args(["-s", &format!("{}x{}", SIZE.0, SIZE.1)])
            .args(["-framerate", self.rate])
            .input_pipe()
            .args(self.video)
            .output_file(&video);
        let mut running = c
            .spawn(Stdio::piped(), Stdio::null(), &CancelToken::new())
            .unwrap();
        let mut stdin = running.take_stdin().unwrap();
        for k in 0..self.frames {
            stdin.write_all(&numbered_frame(SIZE.0, SIZE.1, k)).unwrap();
        }
        drop(stdin);
        running.wait().unwrap();

        let listed = opened(&video).frames;
        let first = listed.time(0).as_f64();
        let made: Vec<f64> = (CLICK_FROM..listed.len())
            .step_by(CLICK_EVERY)
            .map(|k| listed.time(k).as_f64() - first)
            .collect();
        let seconds = listed.end().as_f64() - first + self.audio_lead.max(0.0) + 0.5;
        let wav = dir.join(format!("{}-clicks.wav", self.name));
        let in_wav: Vec<f64> = made.iter().map(|t| t + self.audio_lead).collect();
        write_clicks(&wav, self.audio_rate, seconds, &in_wav);

        let path = dir.join(format!("{}.{}", self.name, self.container));
        let mut c = ToolCommand::ffmpeg(&ffmpeg());
        if self.audio_lead > 0.0 {
            c.args(["-itsoffset", &self.audio_lead.to_string()]);
        }
        c.args(self.video_input).input_file(&video);
        if self.audio_lead < 0.0 {
            c.args(["-itsoffset", &(-self.audio_lead).to_string()]);
        }
        c.input_file(&wav)
            .args(["-map", "0:v", "-map", "1:a", "-c:v", "copy"])
            .args(["-c:a", self.audio_codec])
            .args(self.mux)
            .output_file(&path);
        c.output(&CancelToken::new()).unwrap();
        let heard = made.into_iter().filter(|&t| t + self.audio_lead >= 0.0);
        (path, heard.collect())
    }
}

/// Mono 16-bit PCM, silent but for a click at each of `at`.
fn write_clicks(path: &Path, rate: u32, seconds: f64, at: &[f64]) {
    let mut samples = vec![0i16; (seconds * f64::from(rate)).round() as usize];
    for &t in at.iter().filter(|&&t| t >= 0.0) {
        let first = (t * f64::from(rate)).round() as usize;
        let length = (CLICK_SECONDS * f64::from(rate)) as usize;
        samples[first..first + length].fill(29_000);
    }
    let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + data.len() as u32).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(rate.to_le_bytes());
    wav.extend((rate * 2).to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend((data.len() as u32).to_le_bytes());
    wav.extend(data);
    std::fs::write(path, wav).unwrap();
}

/// When each click starts, in seconds from the start FFmpeg gives the file (its earliest
/// stream), with sound that starts later padded from there. Clicks are at least 0.1 s apart.
fn clicks(path: &Path) -> Vec<f64> {
    let mut c = ToolCommand::ffmpeg(&ffmpeg());
    c.input_file(path)
        .args(["-map", "0:a:0", "-af", "aresample=async=1:first_pts=0"])
        .args(["-ac", "1", "-ar", "48000", "-f", "s16le"])
        .output_pipe();
    let pcm = c.output(&CancelToken::new()).unwrap();
    let mut out: Vec<f64> = Vec::new();
    let mut last = None;
    for (i, s) in pcm.as_chunks::<2>().0.iter().enumerate() {
        if i16::from_le_bytes(*s).unsigned_abs() > 8000 && last.is_none_or(|l| i - l > 4800) {
            out.push(i as f64 / 48_000.0);
            last = Some(i);
        }
    }
    out
}

/// When each frame starts, from the same start as [`clicks`].
fn frame_times(source: &Source) -> Vec<f64> {
    (0..source.frames.len())
        .map(|k| source.frames.time(k).as_f64() - source.spec.container_start_seconds)
        .collect()
}

/// A mask in the bottom right corner, away from the frame number.
fn corner_mask((width, height): (u32, u32)) -> Rect {
    Rect::new(width as f32 - 24.0, height as f32 - 24.0, 16.0, 16.0).unwrap()
}

/// Finds the corner of every frame it is given as a face. The sources are analyzed at their own
/// size, in display orientation.
struct Corner;

impl Detector for Corner {
    fn detect(&mut self, image: &Rgb8Image, _: &Thresholds) -> Result<Found, DetectError> {
        let bbox = corner_mask((image.width, image.height));
        Ok(Found {
            detections: vec![Detection::new(ModelId::Yolo5FaceN, 0.9, bbox, None, None).unwrap()],
            ..Found::default()
        })
    }
}

const EXPORTS: [(OutputCodec, bool); 2] = [(OutputCodec::H264, true), (OutputCodec::Hevc, false)];

/// `input` through the runner in automatic mode with the corner tracked, published as
/// `out/<name>.mp4`.
fn export(input: &Path, name: &str, codec: OutputCodec, hardware_encode: bool) -> PathBuf {
    let destination = input
        .parent()
        .unwrap()
        .join("out")
        .join(format!("{name}.mp4"));
    let job = Job {
        items: vec![JobItem {
            id: ItemId(1),
            input: input.to_path_buf(),
            destination: destination.clone(),
            start: Start::Fresh {
                needs_review: false,
            },
        }],
        review: ReviewMode::Off,
        workers: 1,
    };
    let settings = VideoSettings {
        codec,
        hardware_encode,
        analysis: AnalysisSettings {
            tiles: false,
            ..AnalysisSettings::default()
        },
        ..VideoSettings::default()
    };
    let processor = Arc::new(VideoProcessor::new(
        Tools {
            ffmpeg: ffmpeg(),
            ffprobe: ffprobe(),
        },
        settings,
        input.parent().unwrap().join("records"),
    ));
    let detector = DetectorHandle::spawn(Box::new(Corner)).unwrap();
    let report = run::start(job, processor, detector).unwrap().wait();
    let outcome = &report.items[0];
    assert_eq!(
        outcome.state,
        ItemState::Published(PublishBasis::AutomaticUnreviewed),
        "{name}: {:?}",
        outcome.failure
    );
    assert!(
        outcome.notes.contains(&Note::Tracked {
            faces: 1,
            plates: 0
        }),
        "{name}: {:?}",
        outcome.notes
    );
    destination
}

/// The frames: output frame `n` is the analysis pass's `shown_at(n)` with the mask over it, at
/// the source's rate and display orientation, and every source frame shows. Returns the source
/// frame each output frame shows.
fn check_frames(what: &str, input: &Path, output: &Path) -> Vec<usize> {
    let source = opened(input);
    let rotation = source.spec.rotation;
    let shown: Vec<usize> = source.frames.shown(&source.spec).collect();
    let analysis = numbers(&decoded(input, &source, Some(320)), rotation);
    assert_eq!(
        analysis,
        (0..source.frames.len() as u32).collect::<Vec<_>>(),
        "{what}: the analysis pass"
    );
    let mut every = shown.clone();
    every.dedup();
    assert_eq!(
        every,
        (0..source.frames.len()).collect::<Vec<_>>(),
        "{what}"
    );

    let out = opened(output);
    assert_eq!(out.spec.rotation, Rotation::None, "{what}");
    assert_eq!(
        out.spec.display_size(),
        source.spec.display_size(),
        "{what}"
    );
    assert_eq!(out.spec.rate, source.spec.rate, "{what}");
    assert!(!out.spec.variable_rate, "{what}");
    let frames = decoded(output, &out, None);
    assert_eq!(frames.len(), shown.len(), "{what}: output frames");
    let wanted: Vec<u32> = shown.iter().map(|&k| analysis[k]).collect();
    assert_eq!(numbers(&frames, rotation), wanted, "{what}");
    let mask = corner_mask(out.spec.display_size());
    let (x, y) = ((mask.x() + 8.0) as u32, (mask.y() + 8.0) as u32);
    for (n, frame) in frames.iter().enumerate() {
        let i = ((y * frame.width + x) * 3) as usize;
        assert!(
            frame.data[i..i + 3].iter().all(|&c| c <= 6),
            "{what}: the mask in frame {n}"
        );
    }
    shown
}

/// The sound: each click the source has inside the video is heard in the output, 1 ms or nearer
/// to its place from the video's start, and as the first output frame showing the click's frame
/// starts, that frame is within half an output frame of where it starts in the source.
fn check_sync(what: &str, input: &Path, made: &[f64], output: &Path, shown: &[usize]) {
    let (source, out) = (opened(input), opened(output));
    let (source_times, output_times) = (frame_times(&source), frame_times(&out));
    let (video_start, output_start) = (source_times[0], output_times[0]);
    let seconds = |frames: usize| {
        frames as f64 * f64::from(source.spec.rate.den()) / f64::from(source.spec.rate.num())
    };
    let length = seconds(shown.len());
    let in_source: Vec<f64> = clicks(input)
        .into_iter()
        .map(|t| t - video_start)
        .filter(|&t| t > -0.001 && t < length)
        .collect();
    let made: Vec<f64> = made.iter().copied().filter(|&t| t < length).collect();
    assert_eq!(in_source.len(), made.len(), "{what}: {in_source:?}");
    for (heard, made) in in_source.iter().zip(&made) {
        assert!((heard - made).abs() <= 0.001, "{what}: {in_source:?}");
    }

    let in_output: Vec<f64> = clicks(output).iter().map(|t| t - output_start).collect();
    assert_eq!(in_output.len(), in_source.len(), "{what}: {in_output:?}");
    let half_frame = seconds(1) / 2.0;
    for (&t_in, &t_out) in in_source.iter().zip(&in_output) {
        assert!(
            (t_out - t_in).abs() <= 0.001,
            "{what}: the click at {t_in:.4} s is heard at {t_out:.4} s"
        );
        let k = source_times.partition_point(|&t| t - video_start <= t_in + 0.001) - 1;
        let n = shown.iter().position(|&s| s == k).unwrap();
        let moved = (output_times[n] - output_start) - (source_times[k] - video_start);
        assert!(
            moved.abs() <= half_frame + 1e-6,
            "{what}: source frame {k} shows from output frame {n}, {moved:+.4} s from its time"
        );
    }
}

fn exports_keep_frames_and_sound(recipe: &Recipe) {
    let dir = TestDir::new();
    let (input, made) = recipe.make(dir.path());
    let (num, den) = recipe.output_rate;
    assert_eq!(opened(&input).spec.rate, Rational::new(num, den).unwrap());
    for (codec, hardware) in EXPORTS {
        let what = format!("{} as {codec:?}", recipe.name);
        let output = export(
            &input,
            &format!("{}-{codec:?}", recipe.name),
            codec,
            hardware,
        );
        let shown = check_frames(&what, &input, &output);
        check_sync(&what, &input, &made, &output, &shown);
    }
}

#[test]
fn a_constant_rate_with_b_frames() {
    exports_keep_frames_and_sound(&Recipe::new("cfr-30"));
}

/// FFmpeg adds a container's start to a seek, and 1.12.1 lost the sound of such files.
#[test]
fn a_container_that_starts_late() {
    exports_keep_frames_and_sound(&Recipe {
        rate: "30000/1001",
        mux: &["-output_ts_offset", "1.25"],
        output_rate: (30000, 1001),
        ..Recipe::new("cfr-2997-offset")
    });
}

/// 0.1% too fast or slow would put the last click 20 ms off.
#[test]
fn twenty_seconds_at_23976_with_44100_hz_sound() {
    exports_keep_frames_and_sound(&Recipe {
        frames: 480,
        rate: "24000/1001",
        audio_rate: 44_100,
        output_rate: (24000, 1001),
        ..Recipe::new("cfr-23976-long")
    });
}

#[test]
fn a_rate_that_halves() {
    exports_keep_frames_and_sound(&Recipe {
        frames: 120,
        video: &[
            "-vf",
            "setpts='if(lt(N,60),N/30/TB,(2+(N-60)/15)/TB)'",
            "-fps_mode",
            "passthrough",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ],
        ..Recipe::new("vfr-30-15")
    });
}

/// FFmpeg guesses 15/1 from the start; 29.97 fps would show every frame too, but drift off the
/// source's 30 fps grid by up to 11 ms.
#[test]
fn a_rate_that_doubles_after_ten_seconds() {
    exports_keep_frames_and_sound(&Recipe {
        frames: 180,
        video: &[
            "-vf",
            "setpts='if(lt(N,150),N/15/TB,(10+(N-150)/30)/TB)'",
            "-fps_mode",
            "passthrough",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ],
        ..Recipe::new("vfr-15-30")
    });
}

/// Timestamps up to 5 ms off a 30 fps grid, as a phone writes them.
#[test]
fn jittered_timestamps() {
    exports_keep_frames_and_sound(&Recipe {
        video: &[
            "-vf",
            "settb=1/90000,setpts='(N/30+0.005*sin(N*1.7))/TB'",
            "-fps_mode",
            "passthrough",
            "-enc_time_base",
            "1/90000",
            "-video_track_timescale",
            "90000",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ],
        ..Recipe::new("vfr-jitter")
    });
}

#[test]
fn sound_that_starts_before_the_video() {
    exports_keep_frames_and_sound(&Recipe {
        audio_lead: 0.4,
        ..Recipe::new("audio-early")
    });
}

#[test]
fn sound_that_starts_after_the_video() {
    exports_keep_frames_and_sound(&Recipe {
        audio_lead: -0.3,
        ..Recipe::new("audio-late")
    });
}

#[test]
fn a_turned_video() {
    exports_keep_frames_and_sound(&Recipe {
        rate: "30000/1001",
        video_input: &["-display_rotation", "90"],
        output_rate: (30000, 1001),
        ..Recipe::new("rotated-90")
    });
}

/// Opus becomes AAC, whose encoder delay the MP4 edit list must hide.
#[test]
fn webm_with_opus_sound() {
    exports_keep_frames_and_sound(&Recipe {
        video: &[
            "-c:v",
            "libvpx-vp9",
            "-deadline",
            "realtime",
            "-cpu-used",
            "8",
        ],
        container: "webm",
        audio_codec: "libopus",
        ..Recipe::new("vp9-opus")
    });
}

/// P-3 `analysis_equals_output`, for every frame of every fixture.
#[test]
fn the_output_shows_the_analysed_frame_in_every_frame() {
    let dir = TestDir::new();
    for name in [
        "P-3/cfr-23976.mp4",
        "P-3/cfr-2997-offset-bframes.mp4",
        "P-3/vfr-30-15.mp4",
        "P-3/rotated-90.mp4",
        "P-5/audio-starts-early.mp4",
    ] {
        let input = dir.path().join(Path::new(name).file_name().unwrap());
        std::fs::copy(fixtures().join("defects").join(name), &input).unwrap();
        for (codec, hardware) in EXPORTS {
            let what = format!("{name} as {codec:?}");
            let stem = format!("{}-{codec:?}", input.file_stem().unwrap().to_string_lossy());
            let output = export(&input, &stem, codec, hardware);
            check_frames(&what, &input, &output);
        }
    }
}
