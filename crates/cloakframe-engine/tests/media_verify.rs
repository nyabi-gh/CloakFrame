//! P-4 on real files: the output the writer makes passes the check; an output a frame short, one
//! cut short after muxing and one carrying metadata do not (fixture `defects/P-4`).

mod common;

use std::path::{Path, PathBuf};

use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::Rgb8Image;
use cloakframe_engine::media::decode::{FrameReader, frame_size};
use cloakframe_engine::media::encode::{Destination, Encoder, FrameWriter, OutputCodec, Quality};
use cloakframe_engine::media::process::ToolCommand;
use cloakframe_engine::media::source;
use cloakframe_engine::media::verify::{self, Expected, VerifyError};
use common::{TestDir, fixtures, required_env};

fn ffmpeg() -> PathBuf {
    required_env("CLOAKFRAME_FFMPEG")
}

fn ffprobe() -> PathBuf {
    required_env("CLOAKFRAME_FFPROBE")
}

/// The P-5 source (641×361, audio longer than the video) written with `frames` output frames;
/// returns what the full output would be.
fn export(out: &Path, frames_written: impl Fn(u64) -> u64) -> Expected {
    let input = fixtures().join("defects/P-5/odd-641x361-audio-longer.mp4");
    let cancel = CancelToken::new();
    let source = source::open(&ffprobe(), &input, &cancel).unwrap();
    let (spec, listed) = (&source.spec, &source.frames);
    let total = listed.output_frames(spec);
    let written = frames_written(total);
    let mut writer = FrameWriter::start(
        &ffmpeg(),
        Encoder::software(OutputCodec::H264),
        Quality::Balanced,
        spec,
        written,
        Some(&input),
        Destination::File(out),
        &cancel,
    )
    .unwrap();
    let mut reader = FrameReader::start(
        &ffmpeg(),
        &input,
        spec,
        listed,
        frame_size(spec, None),
        &cancel,
    )
    .unwrap();
    let mut frame = Rgb8Image {
        width: 0,
        height: 0,
        data: Vec::new(),
    };
    let mut read = 0;
    for k in listed.shown(spec).take(written as usize) {
        while read <= k {
            assert!(reader.next_into(&mut frame).unwrap());
            read += 1;
        }
        writer.write(&frame).unwrap();
    }
    writer.finish().unwrap();
    Expected::of(spec, OutputCodec::H264, total)
}

fn check(path: &Path, expected: &Expected) -> Result<(), VerifyError> {
    verify::check(&ffprobe(), &ffmpeg(), path, expected, &CancelToken::new())
}

#[test]
fn the_writers_output_passes() {
    let dir = TestDir::new();
    let out = dir.path().join("out.mp4");
    let expected = export(&out, |n| n);
    assert_eq!(expected.size, (642, 362));
    assert_eq!(expected.audio.len(), 1);
    check(&out, &expected).unwrap();
}

/// A frame short is within the duration's tolerance of a frame; the decoded count catches it.
#[test]
fn an_output_a_frame_short_is_refused() {
    let dir = TestDir::new();
    let out = dir.path().join("out.mp4");
    let expected = export(&out, |n| n - 1);
    match check(&out, &expected) {
        Err(VerifyError::FrameCount {
            expected: e,
            decoded,
        }) => {
            assert_eq!(decoded + 1, e);
            assert_eq!(e, expected.frames);
        }
        other => panic!("{other:?}"),
    }
}

/// Without `-xerror` this file decodes with exit status 0, a frame short.
#[test]
fn an_output_cut_short_after_muxing_is_refused() {
    let dir = TestDir::new();
    let out = dir.path().join("out.mp4");
    let expected = export(&out, |n| n);
    let bytes = std::fs::read(&out).unwrap();
    std::fs::write(&out, &bytes[..bytes.len() - 4096]).unwrap();
    assert!(
        matches!(check(&out, &expected), Err(VerifyError::Decode(_))),
        "{:?}",
        check(&out, &expected)
    );
}

#[test]
fn an_output_with_metadata_is_refused() {
    let dir = TestDir::new();
    let clean = dir.path().join("clean.mp4");
    let expected = export(&clean, |n| n);
    let tagged = dir.path().join("tagged.mp4");
    let mut command = ToolCommand::ffmpeg(&ffmpeg());
    command
        .input_file(&clean)
        .args(["-map", "0", "-c", "copy"])
        .args(["-metadata", "location=+37.5665+126.9780/"])
        .output_file(&tagged);
    command.output(&CancelToken::new()).unwrap();
    assert!(
        matches!(check(&tagged, &expected), Err(VerifyError::Metadata(ref key)) if key == "location"),
        "{:?}",
        check(&tagged, &expected)
    );
}
