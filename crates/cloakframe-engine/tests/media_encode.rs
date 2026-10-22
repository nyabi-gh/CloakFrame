//! Output frames through the pinned FFmpeg: D1's encode side (the source's tags on the output
//! and its colours kept), P-5 (even size by edge padding, audio cut to the video's interval) and
//! audio taken from the video's start when the container starts elsewhere.

mod common;

use std::path::{Path, PathBuf};
use std::time::Instant;

use cloakframe_core::timeline::{SourceFrames, TimelineSpec};
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::Rgb8Image;
use cloakframe_engine::media::decode::{FrameReader, frame_size};
use cloakframe_engine::media::encode::{
    Destination, EncodeError, Encoder, Encoders, FrameWriter, OutputCodec, Quality,
};
use cloakframe_engine::media::process::ToolCommand;
use cloakframe_engine::media::source;
use common::{TestDir, fixtures, required_env};
use serde_json::Value;

fn ffmpeg() -> PathBuf {
    required_env("CLOAKFRAME_FFMPEG")
}

fn ffprobe() -> PathBuf {
    required_env("CLOAKFRAME_FFPROBE")
}

fn defect(name: &str) -> PathBuf {
    fixtures().join("defects").join(name)
}

fn opened(path: &Path) -> (TimelineSpec, SourceFrames) {
    let source = source::open(&ffprobe(), path, &CancelToken::new()).unwrap();
    (source.spec, source.frames)
}

fn decoded(path: &Path, spec: &TimelineSpec, listed: &SourceFrames) -> Vec<Rgb8Image> {
    let mut reader = FrameReader::start(
        &ffmpeg(),
        path,
        spec,
        listed,
        frame_size(spec, None),
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

/// The source re-encoded unmasked with software, its audio kept, as stage 4 will export it.
fn export(source: &Path, out: &Path, encoder: Encoder) -> TimelineSpec {
    let (spec, listed) = opened(source);
    let frames = decoded(source, &spec, &listed);
    let count = listed.output_frames(&spec);
    let mut writer = FrameWriter::start(
        &ffmpeg(),
        encoder,
        Quality::High,
        &spec,
        count,
        Some(source),
        Destination::File(out),
        &CancelToken::new(),
    )
    .unwrap();
    for k in listed.shown(&spec) {
        writer.write(&frames[k]).unwrap();
    }
    writer.finish().unwrap();
    spec
}

fn software() -> Encoder {
    Encoder::software(OutputCodec::H264)
}

fn probe_json(path: &Path, entries: &str) -> Value {
    let mut c = ToolCommand::ffprobe(&ffprobe());
    c.args(["-show_entries", entries, "-of", "json"])
        .input_file(path);
    serde_json::from_slice(&c.output(&CancelToken::new()).unwrap()).unwrap()
}

fn stream<'a>(report: &'a Value, kind: &str) -> &'a Value {
    report["streams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["codec_type"] == kind)
        .unwrap_or_else(|| panic!("no {kind} stream"))
}

fn seconds(value: &Value) -> f64 {
    value.as_str().unwrap().parse().unwrap()
}

/// The audio as decoded by FFmpeg (edit lists applied), mono at 48 kHz.
fn samples(path: &Path) -> Vec<i16> {
    let mut c = ToolCommand::ffmpeg(&ffmpeg());
    c.input_file(path)
        .args(["-map", "0:a:0", "-ac", "1", "-ar", "48000", "-f", "s16le"])
        .output_pipe();
    c.output(&CancelToken::new())
        .unwrap()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&b| i16::from_le_bytes(b))
        .collect()
}

/// When each click starts, in seconds; clicks are at least 0.1 s apart.
fn clicks(samples: &[i16]) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    let mut last = None;
    for (i, &s) in samples.iter().enumerate() {
        if s.unsigned_abs() > 8000 && last.is_none_or(|l| i - l > 4800) {
            out.push(i as f64 / 48_000.0);
            last = Some(i);
        }
    }
    out
}

fn assert_near(got: &[f64], want: &[f64], tolerance: f64, what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: {got:?}");
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() <= tolerance, "{what}: {got:?}");
    }
}

/// D1 `output_tags_equal_source` and `center_rgb`.
#[test]
fn the_output_keeps_the_sources_colour_description() {
    let dir = TestDir::new();
    for name in [
        "D1/bt709-tv-1080p.mp4",
        "D1/bt709-pc-1080p.mp4",
        "D1/bt601-tv-480p.mp4",
    ] {
        let source = defect(name);
        let out = dir.path().join(source.file_name().unwrap());
        export(&source, &out, software());
        let fields = "stream=codec_type,color_space,color_primaries,color_transfer,color_range";
        let tags = |path: &Path| {
            let report = probe_json(path, fields);
            let video = stream(&report, "video").clone();
            [
                "color_space",
                "color_primaries",
                "color_transfer",
                "color_range",
            ]
            .map(|f| video[f].clone())
        };
        assert_eq!(tags(&out), tags(&source), "{name}");
        let center = |path: &Path| {
            let (spec, listed) = opened(path);
            let frame = decoded(path, &spec, &listed).swap_remove(0);
            let i = ((frame.height / 2 * frame.width + frame.width / 2) * 3) as usize;
            frame.data[i..i + 3].to_vec()
        };
        let (want, got) = (center(&source), center(&out));
        for (g, w) in got.iter().zip(&want) {
            assert!(g.abs_diff(*w) <= 3, "{name}: {got:?} for {want:?}");
        }
    }
}

/// P-5 `odd-641x361-audio-longer`.
#[test]
fn an_odd_size_is_padded_and_the_longer_audio_is_cut() {
    let dir = TestDir::new();
    let source = defect("P-5/odd-641x361-audio-longer.mp4");
    let out = dir.path().join("out.mp4");
    export(&source, &out, software());

    let (spec, listed) = opened(&out);
    assert_eq!(spec.display_size(), (642, 362));
    let (source_spec, source_listed) = opened(&source);
    let before = decoded(&source, &source_spec, &source_listed).swap_remove(30);
    let after = decoded(&out, &spec, &listed).swap_remove(30);
    let at = |frame: &Rgb8Image, x: u32, y: u32| {
        let i = ((y * frame.width + x) * 3) as usize;
        frame.data[i..i + 3].to_vec()
    };
    let close = |a: &[u8], b: &[u8]| a.iter().zip(b).all(|(a, b)| a.abs_diff(*b) <= 12);
    for (x, y) in [(0, 0), (320, 180), (640, 100), (100, 360), (640, 360)] {
        assert!(
            close(&at(&after, x, y), &at(&before, x, y)),
            "({x}, {y}): {:?} for {:?}",
            at(&after, x, y),
            at(&before, x, y)
        );
    }
    for y in [0, 100, 360] {
        assert!(
            close(&at(&after, 641, y), &at(&before, 640, y)),
            "right edge at {y}"
        );
    }
    for x in [0, 320, 640] {
        assert!(
            close(&at(&after, x, 361), &at(&before, x, 360)),
            "bottom edge at {x}"
        );
    }

    let report = probe_json(&out, "stream=codec_type,duration");
    let video = seconds(&stream(&report, "video")["duration"]);
    let audio = seconds(&stream(&report, "audio")["duration"]);
    assert!(
        (audio - video).abs() <= 0.001,
        "audio {audio} s, video {video} s"
    );
    let heard = samples(&out);
    assert!(
        (heard.len() as f64 / 48_000.0 - 2.0).abs() <= 0.001,
        "{} samples",
        heard.len()
    );
    assert_near(&clicks(&heard), &[0.0, 0.5, 1.0, 1.5], 0.005, "clicks");
}

/// P-5 `audio-starts-early`: the video starts at 0.4 s, the audio at 0.
#[test]
fn audio_that_starts_early_is_cut_at_the_videos_start() {
    let dir = TestDir::new();
    let out = dir.path().join("out.mp4");
    export(&defect("P-5/audio-starts-early.mp4"), &out, software());
    let report = probe_json(&out, "stream=codec_type,start_time");
    for kind in ["video", "audio"] {
        assert_eq!(seconds(&stream(&report, kind)["start_time"]), 0.0, "{kind}");
    }
    assert_near(
        &clicks(&samples(&out)),
        &[0.1, 0.6, 1.1, 1.6],
        0.02,
        "clicks",
    );
}

/// A file whose container starts before its video: FFmpeg adds the container's start to `-ss`,
/// so 1.12.1's cut at the video's own start time lost the audio altogether.
#[test]
fn the_audio_is_cut_from_the_videos_start_when_the_container_starts_elsewhere() {
    let dir = TestDir::new();
    let source = dir.path().join("offset.mp4");
    let mut c = ToolCommand::ffmpeg(&ffmpeg());
    c.args([
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=160x120:r=30:d=2",
        "-f",
        "lavfi",
        "-i",
        "aevalsrc=if(lt(mod(t\\,0.5)\\,0.002)\\,0.9\\,0):s=48000:d=2.5",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-c:a",
        "aac",
        "-output_ts_offset",
        "10",
    ])
    .output_file(&source);
    c.output(&CancelToken::new()).unwrap();
    let (spec, _) = opened(&source);
    assert_eq!(spec.start.as_f64(), 10.0);
    assert!(spec.container_start_seconds < 10.0, "{spec:?}");

    let out = dir.path().join("out.mp4");
    export(&source, &out, software());
    let heard = samples(&out);
    assert!((heard.len() as f64 / 48_000.0 - 2.0).abs() <= 0.001);
    assert_near(&clicks(&heard), &[0.0, 0.5, 1.0, 1.5], 0.005, "clicks");
}

/// Audio from long before the video: the cut keeps the click on the video's first frame, and of
/// the sound before it only the packets the decoder needs.
#[test]
fn audio_long_before_the_video_is_dropped_but_for_the_decoders_packets() {
    let dir = TestDir::new();
    let audio = dir.path().join("clicks.m4a");
    let mut c = ToolCommand::ffmpeg(&ffmpeg());
    c.args([
        "-f",
        "lavfi",
        "-i",
        "aevalsrc=if(lt(mod(t\\,0.5)\\,0.002)\\,0.9\\,0):s=48000:d=4",
        "-c:a",
        "aac",
    ])
    .output_file(&audio);
    c.output(&CancelToken::new()).unwrap();
    let source = dir.path().join("late-video.mp4");
    let mut c = ToolCommand::ffmpeg(&ffmpeg());
    c.args([
        "-itsoffset",
        "1.5",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=160x120:r=30:d=2",
    ])
    .input_file(&audio)
    .args([
        "-map",
        "0",
        "-map",
        "1",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
    ])
    .args(["-c:a", "copy"])
    .output_file(&source);
    c.output(&CancelToken::new()).unwrap();
    assert_eq!(opened(&source).0.start.as_f64(), 1.5);

    let out = dir.path().join("out.mp4");
    export(&source, &out, software());
    assert_near(
        &clicks(&samples(&out)),
        &[0.0, 0.5, 1.0, 1.5],
        0.005,
        "clicks",
    );
    let mut c = ToolCommand::ffprobe(&ffprobe());
    c.args([
        "-select_streams",
        "a",
        "-show_entries",
        "packet=pts_time",
        "-of",
        "json",
    ])
    .input_file(&out);
    let report: Value = serde_json::from_slice(&c.output(&CancelToken::new()).unwrap()).unwrap();
    let first = seconds(&report["packets"][0]["pts_time"]);
    assert!(
        (-0.3..=0.0).contains(&first),
        "first audio packet at {first} s"
    );
}

/// A two-letter language from WebM is stored in the form MP4 keeps, and audio MP4 cannot copy
/// becomes AAC.
#[test]
fn webm_audio_is_reencoded_with_its_language() {
    let dir = TestDir::new();
    let source = dir.path().join("talk.webm");
    let mut c = ToolCommand::ffmpeg(&ffmpeg());
    c.args([
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=160x120:r=30:d=1",
        "-f",
        "lavfi",
        "-i",
        "sine=d=1.5",
        "-c:v",
        "libvpx",
        "-c:a",
        "libopus",
        "-metadata:s:a:0",
        "language=en",
    ])
    .output_file(&source);
    c.output(&CancelToken::new()).unwrap();
    let out = dir.path().join("out.mp4");
    export(&source, &out, software());
    let report = probe_json(
        &out,
        "stream=codec_type,codec_name,duration:stream_tags=language",
    );
    let audio = stream(&report, "audio");
    assert_eq!(audio["codec_name"], "aac");
    assert_eq!(audio["tags"]["language"], "eng");
    assert!(
        (seconds(&audio["duration"]) - 1.0).abs() <= 0.001,
        "{audio}"
    );
}

#[test]
fn hardware_off_picks_software_and_any_pick_encodes() {
    let path = defect("P-5/odd-641x361-audio-longer.mp4");
    let (spec, _) = opened(&path);
    let cancel = CancelToken::new();
    for codec in [OutputCodec::H264, OutputCodec::Hevc] {
        assert_eq!(
            Encoders::new(false)
                .pick(&ffmpeg(), codec, &spec, &cancel)
                .unwrap(),
            Encoder::software(codec)
        );
        // On runners without an encoder device the hardware candidates fail their trial.
        let picked = Encoders::new(true)
            .pick(&ffmpeg(), codec, &spec, &cancel)
            .unwrap();
        println!("{codec:?}: {picked:?}");
        let dir = TestDir::new();
        let out = dir.path().join("out.mp4");
        export(&path, &out, picked);
        let report = probe_json(&out, "stream=codec_type,codec_name,width,height");
        let video = stream(&report, "video");
        assert_eq!(
            (video["width"].as_u64(), video["height"].as_u64()),
            (Some(642), Some(362))
        );
    }
}

#[test]
fn an_existing_file_is_never_replaced() {
    let dir = TestDir::new();
    let out = dir.write_bytes("taken.mp4", b"someone's video");
    let (spec, _) = opened(&defect("P-5/audio-starts-early.mp4"));
    let started = FrameWriter::start(
        &ffmpeg(),
        software(),
        Quality::Balanced,
        &spec,
        1,
        None,
        Destination::File(&out),
        &CancelToken::new(),
    );
    assert!(
        matches!(started, Err(EncodeError::Output(ref e)) if e.kind() == std::io::ErrorKind::AlreadyExists),
        "{:?}",
        started.err()
    );
    assert_eq!(std::fs::read(&out).unwrap(), b"someone's video");
}

#[test]
fn a_frame_of_another_size_is_refused() {
    let path = defect("P-5/audio-starts-early.mp4");
    let (spec, _) = opened(&path);
    let mut writer = FrameWriter::start(
        &ffmpeg(),
        software(),
        Quality::Balanced,
        &spec,
        1,
        None,
        Destination::Discard,
        &CancelToken::new(),
    )
    .unwrap();
    let small = Rgb8Image {
        width: 2,
        height: 2,
        data: vec![0; 12],
    };
    assert!(writer.write(&small).is_err());
}

/// Plan stage 3: the encode side of the stdin pipe with the software encoder an export uses,
/// measured on every OS like the decode side. Every frame is the same, so the encoder has little
/// to do and the pipe dominates. Run with `--ignored --nocapture`.
#[test]
#[ignore = "a measurement, run on its own"]
fn encode_throughput_1080p() {
    let dir = TestDir::new();
    let path = dir.path().join("1080p.mp4");
    let mut c = ToolCommand::ffmpeg(&ffmpeg());
    c.args([
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=1920x1080:r=30:d=1",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
    ])
    .output_file(&path);
    c.output(&CancelToken::new()).unwrap();
    let (spec, listed) = opened(&path);
    let frame = decoded(&path, &spec, &listed).swap_remove(0);
    let frames = 300;
    let start = Instant::now();
    let mut writer = FrameWriter::start(
        &ffmpeg(),
        software(),
        Quality::Balanced,
        &spec,
        frames,
        None,
        Destination::Discard,
        &CancelToken::new(),
    )
    .unwrap();
    for _ in 0..frames {
        writer.write(&frame).unwrap();
    }
    writer.finish().unwrap();
    let seconds = start.elapsed().as_secs_f64();
    let bytes = frames as f64 * f64::from(frame.width * frame.height * 3);
    println!(
        "encode 1080p rgb24 through stdin (libx264 medium): {frames} frames in {seconds:.2} s, {:.1} fps, {:.0} MB/s",
        frames as f64 / seconds,
        bytes / seconds / 1e6
    );
}
