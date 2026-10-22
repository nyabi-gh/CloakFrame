//! `cloakframe-cli run --videos` end to end with fixed masks (M4): a video and a photo in one
//! run, the odd-sized video padded and noted (P-5), masked in every frame and checked before it
//! is published (P-4); a turned video in HEVC from software; and the options videos need.
//!
//! Needs `CLOAKFRAME_FFMPEG`, `CLOAKFRAME_FFPROBE`, `CLOAKFRAME_ORT_LIBRARY` and
//! `CLOAKFRAME_TEST_ASSETS`.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use cloakframe_core::timeline::Rotation;
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::Rgb8Image;
use cloakframe_engine::media::decode::{FrameReader, frame_size};
use cloakframe_engine::media::source;
use common::{TestDir, fixtures, required_env};

fn ffmpeg() -> PathBuf {
    required_env("CLOAKFRAME_FFMPEG")
}

fn ffprobe() -> PathBuf {
    required_env("CLOAKFRAME_FFPROBE")
}

struct Fixture {
    root: PathBuf,
    _dir: TestDir,
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Fixture {
    fn new() -> Self {
        let dir = TestDir::new();
        let root = dir.mkdir("in").parent().unwrap().to_path_buf();
        Self { root, _dir: dir }
    }

    fn input(&self, name: &str, from: &Path) -> PathBuf {
        let path = self.root.join("in").join(name);
        std::fs::copy(from, &path).unwrap();
        path
    }

    fn out(&self) -> PathBuf {
        self.root.join("out")
    }

    fn cli(&self, args: &[&str]) -> Output {
        let output = Command::new(env!("CARGO_BIN_EXE_cloakframe-cli"))
            .args(args)
            .arg("--sessions")
            .arg(self.root.join("sessions"))
            .env("HOME", self.root.join("home"))
            .env_remove("XDG_CACHE_HOME")
            .output()
            .unwrap();
        Output {
            code: output.status.code().unwrap(),
            stdout: String::from_utf8(output.stdout).unwrap(),
            stderr: String::from_utf8(output.stderr).unwrap(),
        }
    }

    /// `run` over the input folder with the tools and models, plus `extra`.
    fn run(&self, extra: &[&str]) -> Output {
        let (library, models) = (
            required_env("CLOAKFRAME_ORT_LIBRARY"),
            required_env("CLOAKFRAME_TEST_ASSETS"),
        );
        let (out, input, ffmpeg, ffprobe) = (self.out(), self.root.join("in"), ffmpeg(), ffprobe());
        let mut args = vec![
            "run",
            "--output",
            out.to_str().unwrap(),
            "--ort-library",
            library.to_str().unwrap(),
            "--models",
            models.to_str().unwrap(),
            "--ffmpeg",
            ffmpeg.to_str().unwrap(),
            "--ffprobe",
            ffprobe.to_str().unwrap(),
        ];
        args.extend(extra);
        args.push(input.to_str().unwrap());
        self.cli(&args)
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Every frame of `path` at its display size.
fn frames(path: &Path) -> (cloakframe_core::timeline::TimelineSpec, Vec<Rgb8Image>) {
    let cancel = CancelToken::new();
    let source = source::open(&ffprobe(), path, &cancel).unwrap();
    let mut reader = FrameReader::start(
        &ffmpeg(),
        path,
        &source.spec,
        &source.frames,
        frame_size(&source.spec, None),
        &cancel,
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
            return (source.spec, out);
        }
        out.push(frame);
    }
}

fn pixel(frame: &Rgb8Image, x: u32, y: u32) -> [u8; 3] {
    let i = (y as usize * frame.width as usize + x as usize) * 3;
    frame.data[i..i + 3].try_into().unwrap()
}

fn close(a: [u8; 3], b: [u8; 3], tolerance: u8) -> bool {
    a.iter().zip(b).all(|(&a, b)| a.abs_diff(b) <= tolerance)
}

#[test]
fn a_video_and_a_photo_are_exported_in_one_run_with_the_video_masked() {
    let f = Fixture::new();
    let source = f.input(
        "odd.mp4",
        &fixtures().join("defects/P-5/odd-641x361-audio-longer.mp4"),
    );
    f.input(
        "astronaut.png",
        &required_env("CLOAKFRAME_TEST_ASSETS").join("astronaut.png"),
    );
    let run = f.run(&["--review", "off", "--videos", "--mask", "100,50,200,100"]);
    assert_eq!(run.code, 0, "{}\n{}", run.stdout, run.stderr);
    let video_line = run
        .stdout
        .lines()
        .find(|l| l.contains("odd.mp4"))
        .expect("the video's line");
    assert!(
        video_line.contains("published (automatic, unreviewed)")
            && video_line.contains("odd size padded to 642×362"),
        "{video_line}"
    );
    assert_eq!(names(&f.out()), ["astronaut.png", "odd.mp4"]);

    let (spec_in, source_frames) = frames(&source);
    let (spec_out, output_frames) = frames(&f.out().join("odd.mp4"));
    assert_eq!(spec_out.display_size(), (642, 362));
    assert_eq!(spec_out.audio.len(), spec_in.audio.len());
    assert_eq!(output_frames.len(), source_frames.len());
    for (source, output) in source_frames.iter().zip(&output_frames) {
        // Inside the mask: the fill's black. Outside it: the source, through a lossy encode.
        assert!(close(pixel(output, 200, 100), [0, 0, 0], 6));
        assert!(close(pixel(output, 600, 300), pixel(source, 600, 300), 24));
        assert!(close(pixel(output, 20, 340), pixel(source, 20, 340), 24));
    }
}

#[test]
fn a_turned_video_is_encoded_in_hevc_by_software_when_asked() {
    let f = Fixture::new();
    let source = f.input("turned.mp4", &fixtures().join("defects/P-3/rotated-90.mp4"));
    let run = f.run(&[
        "--review",
        "off",
        "--videos",
        "--video-codec",
        "hevc",
        "--hardware-encode",
        "off",
        "--mask",
        "0,0,8,8",
    ]);
    assert_eq!(run.code, 0, "{}\n{}", run.stdout, run.stderr);
    let (spec_in, _) = frames(&source);
    let (spec_out, output_frames) = frames(&f.out().join("turned.mp4"));
    assert_ne!(spec_in.rotation, Rotation::None);
    assert_eq!(spec_out.rotation, Rotation::None);
    assert_eq!(spec_out.display_size(), spec_in.display_size());
    assert!(close(pixel(&output_frames[0], 2, 2), [0, 0, 0], 6));
    let probe = Command::new(ffprobe())
        .args(["-v", "error", "-select_streams", "v:0"])
        .args(["-show_entries", "stream=codec_name", "-of", "csv=p=0"])
        .arg(f.out().join("turned.mp4"))
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(probe.stdout).unwrap().trim(), "hevc");
}

#[test]
fn a_mask_outside_a_video_fails_only_that_video() {
    let f = Fixture::new();
    f.input(
        "odd.mp4",
        &fixtures().join("defects/P-5/odd-641x361-audio-longer.mp4"),
    );
    f.input(
        "astronaut.png",
        &required_env("CLOAKFRAME_TEST_ASSETS").join("astronaut.png"),
    );
    let run = f.run(&["--review", "off", "--videos", "--mask", "700,10,20,20"]);
    assert_eq!(run.code, 1, "{}\n{}", run.stdout, run.stderr);
    assert!(
        run.stdout.lines().any(|l| l.contains("odd.mp4")
            && l.contains("failed")
            && l.contains("mask 1 lies outside the 641×361 video")),
        "{}",
        run.stdout
    );
    assert_eq!(names(&f.out()), ["astronaut.png"]);
}

#[test]
fn videos_need_masks_tools_and_automatic_mode() {
    let f = Fixture::new();
    let usage = |args: &[&str], message: &str| {
        let run = f.run(args);
        assert_eq!(run.code, 2, "{args:?}: {}", run.stderr);
        assert!(run.stderr.contains(message), "{args:?}: {}", run.stderr);
    };
    usage(
        &["--videos", "--mask", "0,0,8,8"],
        "--videos needs --review off",
    );
    usage(
        &["--videos", "--review", "off"],
        "--videos needs at least one --mask",
    );
    usage(&["--mask", "0,0,8,8"], "the video options need --videos");
    usage(&["--videos", "--mask", "0,0,8"], "--mask is X,Y,W,H");
    usage(
        &["--videos", "--mask", "0,0,-8,8"],
        "width or height is negative",
    );
    let run = f.cli(&[
        "run",
        "--output",
        f.out().to_str().unwrap(),
        "--review",
        "off",
        "--videos",
        "--mask",
        "0,0,8,8",
        f.root.join("in").to_str().unwrap(),
    ]);
    assert_eq!(run.code, 2);
    assert!(
        run.stderr.contains("--videos needs --ffmpeg FILE"),
        "{}",
        run.stderr
    );
}
