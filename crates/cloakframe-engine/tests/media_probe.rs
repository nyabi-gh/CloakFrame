//! The pinned ffprobe's reports on the P-3, D1 and P-5 fixtures and on inputs the pinned FFmpeg
//! makes here, read into `TimelineSpec`.

mod common;

use std::path::{Path, PathBuf};

use cloakframe_core::timeline::{
    Matrix, Primaries, Range, Rational, Rotation, Seconds, TimelineSpec, Transfer, VideoCodec,
};
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::media::probe::{ProbeError, Unsupported, probe};
use cloakframe_engine::media::process::ToolCommand;
use common::{TestDir, fixtures, required_env};

fn probed(path: &Path) -> Result<TimelineSpec, ProbeError> {
    probe(
        &required_env("CLOAKFRAME_FFPROBE"),
        path,
        &CancelToken::new(),
    )
}

fn fixture(defect: &str, name: &str) -> TimelineSpec {
    probed(&fixtures().join("defects").join(defect).join(name)).unwrap()
}

fn rate(num: u32, den: u32) -> Rational {
    Rational::new(num, den).unwrap()
}

#[test]
fn constant_rate_fixtures_keep_their_rate_and_start() {
    let s = fixture("P-3", "cfr-23976.mp4");
    assert_eq!((s.rate, s.variable_rate), (rate(24000, 1001), false));
    assert_eq!((s.start, s.estimated_frames), (Seconds::ZERO, 72));

    let s = fixture("P-3", "cfr-2997-offset-bframes.mp4");
    assert_eq!((s.rate, s.estimated_frames), (rate(30000, 1001), 90));
    assert_eq!(s.start, Seconds::new(5, 4).unwrap());
    assert_eq!(s.frame_start(1), Seconds::new(38501, 30000).unwrap());
}

/// 1.12.1 used the average rate (600/29) here and dropped source frames 1, 4, 8, 11…
#[test]
fn the_variable_rate_fixture_runs_at_its_nominal_rate() {
    let s = fixture("P-3", "vfr-30-15.mp4");
    assert_eq!((s.rate, s.variable_rate), (rate(30, 1), true));
    assert_eq!(s.estimated_frames, 87);
}

#[test]
fn the_rotated_fixture_is_seen_upright() {
    let s = fixture("P-3", "rotated-90.mp4");
    assert_eq!(s.rotation, Rotation::ThreeQuarters);
    assert_eq!((s.coded_width, s.coded_height), (640, 360));
    assert_eq!(s.display_size(), (360, 640));
}

#[test]
fn an_early_audio_start_moves_only_the_video_start() {
    let s = fixture("P-5", "audio-starts-early.mp4");
    assert_eq!(s.start, Seconds::new(2, 5).unwrap());
    assert_eq!(s.audio.len(), 1);
    assert_eq!((s.audio[0].index, s.audio[0].codec.as_str()), (1, "aac"));
    assert_eq!(s.audio[0].end.map(Seconds::as_f64), Some(2.4));

    let s = fixture("P-5", "odd-641x361-audio-longer.mp4");
    assert_eq!((s.coded_width, s.coded_height), (641, 361));
}

/// D1: the source's tags are taken as they are; untagged fields follow the frame height.
#[test]
fn colour_tags_come_from_the_source_or_the_height_rule() {
    let s = fixture("D1", "bt709-pc-1080p.mp4");
    assert_eq!(
        (
            s.color.matrix,
            s.color.primaries,
            s.color.transfer,
            s.color.range
        ),
        (Matrix::Bt709, Primaries::Bt709, Transfer::Bt709, Range::Pc)
    );
    assert!(!s.color.assumed);
    let s = fixture("D1", "bt601-tv-480p.mp4");
    assert_eq!(
        (s.color.matrix, s.color.range, s.color.assumed),
        (Matrix::Smpte170m, Range::Tv, false)
    );
    let s = fixture("P-3", "cfr-23976.mp4");
    assert_eq!(
        (s.color.matrix, s.color.primaries, s.color.assumed),
        (Matrix::Smpte170m, Primaries::Smpte170m, true)
    );
}

/// Makes `name` with the pinned FFmpeg from a one-second test pattern and these output options.
fn generated(dir: &TestDir, name: &str, inputs: &[&str], options: &[&str]) -> PathBuf {
    let path = dir.path().join(name);
    let mut c = ToolCommand::ffmpeg(&required_env("CLOAKFRAME_FFMPEG"));
    for input in inputs {
        c.args(["-f", "lavfi", "-i", input]);
    }
    c.args(options).output_file(&path);
    c.output(&CancelToken::new()).unwrap();
    path
}

const PATTERN: &str = "testsrc=s=64x48:d=1";

fn refused(path: &Path) -> Unsupported {
    match probed(path) {
        Err(ProbeError::Unsupported(u)) => u,
        other => panic!("{other:?}"),
    }
}

/// D1: tags ffprobe names as the encoder options do (`bt470bg` for transfer 5) or that the
/// height rule never assumes (Display P3) stay the source's; a matrix the decoder's scaler cannot
/// take refuses the video instead of being decoded as another one.
#[test]
fn every_tag_ffprobe_reports_is_read_or_refused() {
    let dir = TestDir::new();
    let tagged = |name: &str, codes: &str| {
        generated(
            &dir,
            name,
            &[PATTERN],
            &[
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-bsf:v",
                &format!("h264_metadata={codes}"),
            ],
        )
    };
    let pal = probed(&tagged(
        "pal.mp4",
        "colour_primaries=5:transfer_characteristics=5:matrix_coefficients=5:video_full_range_flag=0",
    ))
    .unwrap();
    assert_eq!(
        (
            pal.color.matrix,
            pal.color.primaries,
            pal.color.transfer,
            pal.color.assumed
        ),
        (
            Matrix::Bt470bg,
            Primaries::Bt470bg,
            Transfer::Bt470bg,
            false
        )
    );
    let p3 = probed(&tagged(
        "p3.mp4",
        "colour_primaries=12:transfer_characteristics=1:matrix_coefficients=1:video_full_range_flag=0",
    ))
    .unwrap();
    assert_eq!(
        (p3.color.primaries, p3.color.assumed),
        (Primaries::Smpte432, false)
    );
    assert_eq!(
        refused(&tagged("ycgco.mp4", "matrix_coefficients=8")),
        Unsupported::Color("ycgco".into())
    );
}

/// An HD picture cropped wide (1280x544) is BT.709 like the players that guess by size.
#[test]
fn an_untagged_wide_hd_crop_is_taken_as_bt709() {
    let dir = TestDir::new();
    let path = generated(
        &dir,
        "crop.mp4",
        &["testsrc=s=1280x544:d=0.2"],
        &["-c:v", "libx264", "-preset", "ultrafast"],
    );
    let s = probed(&path).unwrap();
    assert_eq!(
        (s.color.matrix, s.color.primaries, s.color.assumed),
        (Matrix::Bt709, Primaries::Bt709, true)
    );
}

/// The spec turns the picture only as FFmpeg will when decoding: by the display matrix rounded to
/// whole degrees, and only for an exact quarter turn; the old `rotate` tag FFmpeg no longer
/// applies.
#[test]
fn rotation_follows_what_ffmpeg_applies() {
    let dir = TestDir::new();
    let plain = generated(&dir, "plain.mp4", &[PATTERN], &["-c:v", "libx264"]);
    let turned = |name: &str, options: &[&str]| {
        let path = dir.path().join(name);
        let mut c = ToolCommand::ffmpeg(&required_env("CLOAKFRAME_FFMPEG"));
        c.args(options)
            .input_file(&plain)
            .args(["-c", "copy"])
            .output_file(&path);
        c.output(&CancelToken::new()).unwrap();
        path
    };
    let nearly = probed(&turned("90.4.mp4", &["-display_rotation", "90.4"])).unwrap();
    assert_eq!(nearly.display_size(), (48, 64));
    assert_eq!(
        refused(&turned("90.6.mp4", &["-display_rotation", "90.6"])),
        Unsupported::Rotation
    );
    assert_eq!(
        refused(&turned("hflip.mp4", &["-display_hflip"])),
        Unsupported::Mirrored
    );
    let tagged = generated(
        &dir,
        "tagged.webm",
        &[PATTERN],
        &["-c:v", "libvpx", "-metadata:s:v:0", "rotate=90"],
    );
    let s = probed(&tagged).unwrap();
    assert_eq!((s.rotation, s.display_size()), (Rotation::None, (64, 48)));
}

#[test]
fn cover_art_is_not_a_second_video() {
    let dir = TestDir::new();
    let path = generated(
        &dir,
        "cover.mp4",
        &["color=red:s=32x32:d=1", PATTERN],
        &[
            "-map",
            "0:v",
            "-map",
            "1:v",
            "-frames:v:0",
            "1",
            "-c:v:0",
            "png",
            "-c:v:1",
            "libx264",
            "-disposition:v:0",
            "attached_pic",
        ],
    );
    let s = probed(&path).unwrap();
    assert_eq!((s.codec, s.other_video_streams), (VideoCodec::H264, 0));
    assert_eq!((s.coded_width, s.coded_height), (64, 48));
}

#[test]
fn a_second_video_stream_is_counted() {
    let dir = TestDir::new();
    let path = generated(
        &dir,
        "two.mp4",
        &[PATTERN, "testsrc2=s=32x32:d=1"],
        &["-map", "0", "-map", "1", "-c:v", "libx264"],
    );
    let s = probed(&path).unwrap();
    assert_eq!((s.stream, s.other_video_streams), (0, 1));
}

#[test]
fn webm_is_taken_and_other_matroska_is_not() {
    let dir = TestDir::new();
    let webm = generated(&dir, "vp9.webm", &[PATTERN], &["-c:v", "libvpx-vp9"]);
    assert_eq!(probed(&webm).unwrap().codec, VideoCodec::Vp9);
    let mkv = generated(&dir, "h264.mkv", &[PATTERN], &["-c:v", "libx264"]);
    assert_eq!(
        refused(&mkv),
        Unsupported::Container("matroska,webm".into())
    );
}

#[test]
fn ten_bit_and_hdr_video_wait_for_m9() {
    let dir = TestDir::new();
    let ten = generated(
        &dir,
        "ten.mp4",
        &[PATTERN],
        &[
            "-c:v",
            "libx265",
            "-pix_fmt",
            "yuv420p10le",
            "-x265-params",
            "log-level=error",
        ],
    );
    assert_eq!(
        refused(&ten),
        Unsupported::PixelFormat("yuv420p10le".into())
    );
    let pq = generated(
        &dir,
        "pq.mp4",
        &[PATTERN],
        &[
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-bsf:v",
            "h264_metadata=colour_primaries=9:transfer_characteristics=16:matrix_coefficients=9",
        ],
    );
    assert_eq!(refused(&pq), Unsupported::Hdr("smpte2084".into()));
}

#[test]
fn a_file_ffprobe_cannot_read_fails_to_probe() {
    let dir = TestDir::new();
    let path = dir.write_bytes("broken.mp4", b"not a video");
    assert!(matches!(probed(&path), Err(ProbeError::Tool(_))));
}

/// The fuzz seeds are these fixtures' reports; a seed the parser refuses would leave the fuzzer
/// exploring only the error paths.
#[test]
fn the_fuzz_seeds_are_reports_the_parser_takes() {
    let seeds =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds/probe_report");
    let mut count = 0;
    for entry in std::fs::read_dir(seeds).unwrap() {
        let path = entry.unwrap().path();
        let spec = cloakframe_engine::media::probe::parse(&std::fs::read(&path).unwrap());
        assert!(spec.is_ok(), "{}: {spec:?}", path.display());
        count += 1;
    }
    assert_eq!(count, 9);
}
