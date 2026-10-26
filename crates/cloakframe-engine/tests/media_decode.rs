//! Source frames through the pinned FFmpeg: the P-3 fixtures (every source frame once, in
//! order; the preview's frame is the pass's frame byte for byte), D1's decode side, and pixels
//! that do not depend on the CPU.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cloakframe_core::timeline::{Rational, SourceFrames, TimelineSpec};
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::Rgb8Image;
use cloakframe_engine::media::decode::{DecodeError, FrameReader, frame_size, source_frame};
use cloakframe_engine::media::process::ToolCommand;
use cloakframe_engine::media::source;
use common::{TestDir, fixtures, frame_number, required_env};
use sha2::{Digest, Sha256};

fn ffmpeg() -> PathBuf {
    required_env("CLOAKFRAME_FFMPEG")
}

fn opened(path: &Path) -> (TimelineSpec, SourceFrames) {
    let source = source::open(
        &required_env("CLOAKFRAME_FFPROBE"),
        path,
        &CancelToken::new(),
    )
    .unwrap();
    (source.spec, source.frames)
}

fn all_frames(
    path: &Path,
    spec: &TimelineSpec,
    listed: &SourceFrames,
    size: (u32, u32),
) -> Vec<Rgb8Image> {
    let mut reader =
        FrameReader::start(&ffmpeg(), path, spec, listed, size, &CancelToken::new()).unwrap();
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

const P3: [&str; 5] = [
    "P-3/cfr-23976.mp4",
    "P-3/cfr-2997-offset-bframes.mp4",
    "P-3/vfr-30-15.mp4",
    "P-3/rotated-90.mp4",
    "P-5/audio-starts-early.mp4",
];

fn defect(name: &str) -> PathBuf {
    fixtures().join("defects").join(name)
}

#[test]
fn a_pass_gives_every_source_frame_once_in_order() {
    for name in P3 {
        let path = defect(name);
        let (spec, listed) = opened(&path);
        let decoded = all_frames(&path, &spec, &listed, frame_size(&spec, None));
        assert_eq!(decoded.len(), listed.len(), "{name}");
        for (k, frame) in decoded.iter().enumerate() {
            assert_eq!(
                frame_number(frame.width, frame.height, &frame.data, spec.rotation),
                k as u32,
                "{name} frame {k}"
            );
        }
    }
}

/// P-3 `vfr`: the 30 → 15 fps fixture keeps all 60 source frames in its constant-rate output.
#[test]
fn the_variable_rate_output_shows_every_source_frame() {
    let (spec, listed) = opened(&defect("P-3/vfr-30-15.mp4"));
    let mut shown: Vec<usize> = listed.shown(&spec).collect();
    assert!(shown.len() > listed.len());
    shown.dedup();
    assert_eq!(shown, (0..listed.len()).collect::<Vec<_>>());
}

/// FFmpeg guesses 15/1 as this file's rate from its first seconds, which would drop half the
/// frames of the 30 fps stretch after them.
#[test]
fn a_faster_stretch_after_the_first_seconds_keeps_every_frame() {
    let dir = TestDir::new();
    let path = dir.path().join("15-then-30.mp4");
    let mut c = ToolCommand::ffmpeg(&ffmpeg());
    c.args([
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=64x48:r=30:d=11",
        "-vf",
        "select='lt(t\\,10)*not(mod(n\\,2))+gte(t\\,10)'",
        "-fps_mode",
        "vfr",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
    ])
    .output_file(&path);
    c.output(&CancelToken::new()).unwrap();
    let (spec, listed) = opened(&path);
    assert_eq!(spec.nominal_rate, Rational::new(15, 1));
    assert_eq!(spec.rate, Rational::new(30, 1).unwrap());
    assert_eq!(listed.len(), 180);
    let mut shown: Vec<usize> = listed.shown(&spec).collect();
    shown.dedup();
    assert_eq!(shown, (0..listed.len()).collect::<Vec<_>>());
}

/// An open GOP cut at a later keyframe without an edit list: the decoder drops the leading
/// pictures that refer to the GOP before, which the packet list still has.
#[test]
fn leading_pictures_the_decoder_drops_are_not_listed() {
    let dir = TestDir::new();
    for (codec, params) in [
        (
            "libx264",
            [
                "-x264-params",
                "keyint=30:min-keyint=30:open-gop=1:bframes=3:scenecut=0",
            ],
        ),
        (
            "libx265",
            [
                "-x265-params",
                "keyint=30:min-keyint=30:open-gop=1:bframes=4:scenecut=0:log-level=error",
            ],
        ),
    ] {
        let whole = dir.path().join(format!("{codec}-open-gop.mp4"));
        let mut c = ToolCommand::ffmpeg(&ffmpeg());
        c.args([
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=64x48:r=30:d=4",
            "-c:v",
            codec,
        ])
        .args(params)
        .args(["-pix_fmt", "yuv420p"])
        .output_file(&whole);
        c.output(&CancelToken::new()).unwrap();
        let cut = dir.path().join(format!("{codec}-cut.mp4"));
        let mut c = ToolCommand::ffmpeg(&ffmpeg());
        c.args(["-ss", "1"])
            .input_file(&whole)
            .args(["-c", "copy", "-use_editlist", "0"])
            .output_file(&cut);
        c.output(&CancelToken::new()).unwrap();

        let (spec, listed) = opened(&cut);
        let size = frame_size(&spec, None);
        let pass = all_frames(&cut, &spec, &listed, size);
        assert_eq!(pass.len(), 90, "{codec}");
        for k in [0, listed.len() - 1] {
            let preview = source_frame(
                &ffmpeg(),
                &cut,
                &spec,
                &listed,
                k,
                size,
                &CancelToken::new(),
            )
            .unwrap();
            assert!(preview == pass[k], "{codec} frame {k}");
        }
    }
}

/// WebM written as it streams (`-f webm pipe:1`, a browser's recorder) gives no duration.
#[test]
fn webm_without_a_duration_opens_from_its_frames() {
    let dir = TestDir::new();
    let mut c = ToolCommand::ffmpeg(&ffmpeg());
    c.args([
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=64x48:r=30:d=2",
        "-c:v",
        "libvpx",
        "-f",
        "webm",
    ])
    .output_pipe();
    let path = dir.path().join("streamed.webm");
    std::fs::write(&path, c.output(&CancelToken::new()).unwrap()).unwrap();
    let (spec, listed) = opened(&path);
    assert_eq!(listed.len(), 60);
    assert_eq!(listed.output_frames(&spec), 60);
    assert!(
        spec.duration_seconds
            .is_some_and(|d| (d - 2.0).abs() < 0.05)
    );
}

/// P-3 `analysis_equals_preview`, for every frame of every fixture.
#[test]
fn the_preview_of_a_frame_is_the_passs_frame() {
    for name in P3 {
        let path = defect(name);
        let (spec, listed) = opened(&path);
        let size = frame_size(&spec, Some(320));
        let pass = all_frames(&path, &spec, &listed, size);
        for (k, expected) in pass.iter().enumerate() {
            let preview = source_frame(
                &ffmpeg(),
                &path,
                &spec,
                &listed,
                k,
                size,
                &CancelToken::new(),
            )
            .unwrap();
            assert!(preview == *expected, "{name} frame {k}");
        }
    }
}

/// P-3 for playback: reading on from frame k, after a seek, gives the pass's frames from k.
#[test]
fn playback_from_a_frame_gives_the_passs_frames_from_it() {
    for name in P3 {
        let path = defect(name);
        let (spec, listed) = opened(&path);
        let size = frame_size(&spec, Some(320));
        let pass = all_frames(&path, &spec, &listed, size);
        for from in [1, pass.len() / 2, pass.len() - 1] {
            let mut reader = FrameReader::start_at(
                &ffmpeg(),
                &path,
                &spec,
                &listed,
                from,
                size,
                &CancelToken::new(),
            )
            .unwrap();
            let mut frame = Rgb8Image {
                width: 0,
                height: 0,
                data: Vec::new(),
            };
            let mut k = from;
            while reader.next_into(&mut frame).unwrap() {
                assert!(frame == pass[k], "{name} from {from}, frame {k}");
                k += 1;
            }
            assert_eq!(k, pass.len(), "{name} from {from}");
        }
    }
}

/// D1, decode side: the fixtures' source colour (0xC83C28) comes back through each file's own
/// matrix and range, tagged or not.
#[test]
fn frames_decode_with_the_specs_matrix_and_range() {
    for name in [
        "D1/bt709-tv-1080p.mp4",
        "D1/bt709-pc-1080p.mp4",
        "D1/bt601-tv-480p.mp4",
    ] {
        let path = defect(name);
        let (spec, listed) = opened(&path);
        let frame = &all_frames(&path, &spec, &listed, frame_size(&spec, None))[0];
        let i = ((frame.height / 2 * frame.width + frame.width / 2) * 3) as usize;
        let center = &frame.data[i..i + 3];
        for (got, want) in center.iter().zip([200u8, 60, 40]) {
            assert!(got.abs_diff(want) <= 3, "{name}: {center:?}");
        }
    }
}

/// Recorded on Apple silicon; CI compares it on x86 Linux and Windows.
const BT709_TV_AT_960: &str = "787491adc129b0abc11c0db3b0aee9a10c023cf42a37b2c4122e5c4e10fa36d3";

#[test]
fn scaled_frames_are_the_same_on_every_cpu() {
    let path = defect("D1/bt709-tv-1080p.mp4");
    let (spec, listed) = opened(&path);
    let size = frame_size(&spec, Some(960));
    assert_eq!(size, (960, 540));
    let frames = all_frames(&path, &spec, &listed, size);
    let mut digest = Sha256::new();
    for frame in &frames {
        digest.update(&frame.data);
    }
    let hex: String = digest
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hex, BT709_TV_AT_960);
}

fn pattern(dir: &TestDir, name: &str, size: &str, seconds: u32) -> PathBuf {
    let path = dir.path().join(name);
    let mut c = ToolCommand::ffmpeg(&ffmpeg());
    c.args([
        "-f",
        "lavfi",
        "-i",
        &format!("testsrc2=s={size}:r=30:d={seconds}"),
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-pix_fmt",
        "yuv420p",
    ])
    .output_file(&path);
    c.output(&CancelToken::new()).unwrap();
    path
}

#[test]
fn cancelling_stops_a_pass() {
    let dir = TestDir::new();
    let path = pattern(&dir, "long.mp4", "320x240", 20);
    let (spec, listed) = opened(&path);
    let cancel = CancelToken::new();
    let mut reader = FrameReader::start(
        &ffmpeg(),
        &path,
        &spec,
        &listed,
        frame_size(&spec, None),
        &cancel,
    )
    .unwrap();
    let mut frame = Rgb8Image {
        width: 0,
        height: 0,
        data: Vec::new(),
    };
    assert!(reader.next_into(&mut frame).unwrap());
    cancel.cancel();
    let start = Instant::now();
    let ended = loop {
        match reader.next_into(&mut frame) {
            Ok(true) => assert!(start.elapsed() < Duration::from_secs(10)),
            other => break other,
        }
    };
    assert!(matches!(ended, Err(DecodeError::Cancelled)), "{ended:?}");
}

/// Plan decision: the stdout pipe on every OS, measured. Run with `--ignored --nocapture`.
#[test]
#[ignore = "a measurement, run on its own"]
fn decode_throughput_1080p() {
    let dir = TestDir::new();
    let path = pattern(&dir, "1080p.mp4", "1920x1080", 10);
    let (spec, listed) = opened(&path);
    let size = frame_size(&spec, None);
    let start = Instant::now();
    let mut reader =
        FrameReader::start(&ffmpeg(), &path, &spec, &listed, size, &CancelToken::new()).unwrap();
    let mut frame = Rgb8Image {
        width: 0,
        height: 0,
        data: Vec::new(),
    };
    let mut decoded = 0;
    while reader.next_into(&mut frame).unwrap() {
        decoded += 1;
    }
    let seconds = start.elapsed().as_secs_f64();
    let bytes = decoded as f64 * f64::from(size.0 * size.1 * 3);
    println!(
        "decode 1080p rgb24 through stdout: {decoded} frames in {seconds:.2} s, {:.1} fps, {:.0} MB/s",
        decoded as f64 / seconds,
        bytes / seconds / 1e6
    );
}
