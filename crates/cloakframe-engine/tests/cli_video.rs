//! `cloakframe-cli run --videos` end to end: a video and a photo in one run with a stand-in
//! detector (`--fixed-detection`, test builds), the odd-sized video padded and noted (P-5), the
//! detected box tracked and masked in every frame and checked before it is published (P-4); a
//! face the pinned model finds in a video, masked; a turned video in HEVC from software with a
//! fixed mask; a video held for review, kept in a session and resumed from it with the
//! reviewer's edits; and the options videos need.
//!
//! Needs `CLOAKFRAME_FFMPEG`, `CLOAKFRAME_FFPROBE`, `CLOAKFRAME_ORT_LIBRARY` and
//! `CLOAKFRAME_TEST_ASSETS`, and the `synthetic` feature.
#![cfg(feature = "synthetic")]

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
        let mut args = vec![
            "--ort-library".to_owned(),
            library.to_str().unwrap().to_owned(),
            "--models".to_owned(),
            models.to_str().unwrap().to_owned(),
        ];
        args.extend(extra.iter().map(|&a| a.to_owned()));
        self.run_with(&args)
    }

    /// `run` over the input folder with the tools, plus `extra`.
    fn run_with(&self, extra: &[String]) -> Output {
        let (out, input, ffmpeg, ffprobe) = (self.out(), self.root.join("in"), ffmpeg(), ffprobe());
        let mut args = vec![
            "run",
            "--output",
            out.to_str().unwrap(),
            "--ffmpeg",
            ffmpeg.to_str().unwrap(),
            "--ffprobe",
            ffprobe.to_str().unwrap(),
        ];
        args.extend(extra.iter().map(String::as_str));
        args.push(input.to_str().unwrap());
        self.cli(&args)
    }

    /// `run` with the stand-in detector finding `found` in every image, without tiles.
    fn run_finding(&self, found: &str, extra: &[&str]) -> Output {
        let mut args: Vec<String> = ["--fixed-detection", found, "--tiles", "off"]
            .iter()
            .map(|&a| a.to_owned())
            .collect();
        args.extend(extra.iter().map(|&a| a.to_owned()));
        self.run_with(&args)
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
    let run = f.run_finding("100,50,200,100", &["--review", "off", "--videos"]);
    assert_eq!(run.code, 0, "{}\n{}", run.stdout, run.stderr);
    let video_line = run
        .stdout
        .lines()
        .find(|l| l.contains("odd.mp4"))
        .expect("the video's line");
    assert!(
        video_line.contains("published (automatic, unreviewed)")
            && video_line.contains("tracked 1 face")
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
        // Inside the box: the fill's black. Outside it and its padding: the source, through a
        // lossy encode.
        for (x, y) in [(100, 50), (200, 100), (299, 149)] {
            assert!(close(pixel(output, x, y), [0, 0, 0], 6), "({x}, {y})");
        }
        assert!(close(pixel(output, 600, 300), pixel(source, 600, 300), 24));
        assert!(close(pixel(output, 20, 340), pixel(source, 20, 340), 24));
    }
}

/// `--shape ellipse` masks photos and videos with the ellipse through the padded box: its
/// corners are left, the box and the padding between are masked.
#[test]
fn the_ellipse_shape_masks_photos_and_videos() {
    let f = Fixture::new();
    let source = f.input(
        "odd.mp4",
        &fixtures().join("defects/P-5/odd-641x361-audio-longer.mp4"),
    );
    let photo = f.input(
        "astronaut.png",
        &required_env("CLOAKFRAME_TEST_ASSETS").join("astronaut.png"),
    );
    let run = f.run_finding(
        "100,50,200,100",
        &["--review", "off", "--videos", "--shape", "ellipse"],
    );
    assert_eq!(run.code, 0, "{}\n{}", run.stdout, run.stderr);
    // The box is 100..300 × 50..150 and padded to 64..336 × 32..168.
    let (masked, left) = ([(100, 50), (299, 149), (200, 40)], [(66, 34), (333, 165)]);
    let photo_in = image::open(&photo).unwrap().into_rgb8();
    let photo_out = image::open(f.out().join("astronaut.png"))
        .unwrap()
        .into_rgb8();
    for (x, y) in masked {
        assert_eq!(photo_out.get_pixel(x, y).0, [0, 0, 0], "({x}, {y})");
    }
    for (x, y) in left {
        assert_eq!(
            photo_out.get_pixel(x, y),
            photo_in.get_pixel(x, y),
            "({x}, {y})"
        );
    }
    let (_, source_frames) = frames(&source);
    let (_, output_frames) = frames(&f.out().join("odd.mp4"));
    for (source, output) in source_frames.iter().zip(&output_frames) {
        for (x, y) in masked {
            assert!(close(pixel(output, x, y), [0, 0, 0], 6), "({x}, {y})");
        }
        for (x, y) in left {
            assert!(
                close(pixel(output, x, y), pixel(source, x, y), 24),
                "({x}, {y})"
            );
        }
    }
}

/// `--videos` detects with the models: the astronaut's face in a video of the photo is found in
/// every frame, tracked and masked.
#[test]
fn a_face_the_model_finds_in_a_video_is_masked() {
    let f = Fixture::new();
    let video = f.root.join("in/astronaut.mp4");
    let status = Command::new(ffmpeg())
        .args(["-v", "error", "-loop", "1", "-i"])
        .arg(required_env("CLOAKFRAME_TEST_ASSETS").join("astronaut.png"))
        .args([
            "-t", "1", "-r", "15", "-c:v", "libx264", "-pix_fmt", "yuv420p",
        ])
        .arg(&video)
        .status()
        .unwrap();
    assert!(status.success());
    let run = f.run(&[
        "--review", "off", "--videos", "--tiles", "off", "--gpu", "off",
    ]);
    assert_eq!(run.code, 0, "{}\n{}", run.stdout, run.stderr);
    assert!(
        run.stdout.contains("published (automatic, unreviewed)")
            && run.stdout.contains("tracked 1 face"),
        "{}",
        run.stdout
    );
    // Where 1.12.1 found the face in the photo at the default threshold.
    let path = fixtures().join("detect/yolo5face/astronaut@0.50.json");
    let want: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let face = &want["detections"][0]["box"];
    let at = |i: usize| face[i].as_f64().unwrap();
    let (x, y) = ((at(0) + at(2) / 2.0) as u32, (at(1) + at(3) / 2.0) as u32);
    let (_, output_frames) = frames(&f.out().join("astronaut.mp4"));
    assert_eq!(output_frames.len(), 15);
    for frame in &output_frames {
        assert!(close(pixel(frame, x, y), [0, 0, 0], 6));
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
    let run = f.run_finding(
        "0,0,1,1",
        &["--review", "off", "--videos", "--mask", "700,10,20,20"],
    );
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

/// A video held for review (here because its output name is taken) is kept in a session with its
/// records and tracks; review renders it again from them with the reviewer's edits: the tracked
/// box left unmasked, a box drawn by hand masked in every frame. The session and its records go
/// once it is published.
#[test]
fn a_video_held_for_review_is_resumed_from_its_session() {
    let f = Fixture::new();
    let source = f.input(
        "odd.mp4",
        &fixtures().join("defects/P-5/odd-641x361-audio-longer.mp4"),
    );
    std::fs::create_dir_all(f.out()).unwrap();
    std::fs::write(f.out().join("odd.mp4"), b"someone else's").unwrap();
    let run = f.run_finding("100,50,200,100", &["--videos"]);
    assert_eq!(run.code, 3, "{}\n{}", run.stdout, run.stderr);
    assert!(run.stdout.contains("needs review"), "{}", run.stdout);
    let id = run
        .stdout
        .lines()
        .find_map(|l| l.strip_prefix("session\t"))
        .expect("a session")
        .to_owned();
    let sessions = f.root.join("sessions");
    let stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(sessions.join(format!("{id}.json"))).unwrap())
            .unwrap();
    let item = &stored["items"][0];
    assert_eq!(item["media"], "video");
    assert_eq!(item["tracks"][0]["kind"], "face");
    assert_eq!(item["tracks"][0]["id"], 1);
    let last = item["frames"].as_u64().unwrap() - 1;
    assert!(sessions.join(&id).join("item-0/regions").exists());

    let decisions = f.root.join("decisions.json");
    let decide = |extra: &str| {
        std::fs::write(
            &decisions,
            format!(
                r#"{{"items": [{{"item": 0, "revision": 0, "action": "approve",
                    "rename_to": "odd (2).mp4"{extra}}}]}}"#
            ),
        )
        .unwrap();
    };
    decide(&format!(
        r#", "excluded_tracks": [1], "added_tracks": [{{"first": 0, "last": {last},
            "keyframes": [{{"frame": 0, "bbox": {{"x": 400, "y": 250, "width": 100, "height": 60}}}}]}}]"#
    ));
    let review = |extra: &[&str]| {
        let (ffmpeg, ffprobe) = (ffmpeg(), ffprobe());
        let mut args = vec![
            "review",
            "--session",
            &id,
            "--decisions",
            decisions.to_str().unwrap(),
            "--tiles",
            "off",
        ];
        if !extra.is_empty() {
            args.extend([
                "--ffmpeg",
                ffmpeg.to_str().unwrap(),
                "--ffprobe",
                ffprobe.to_str().unwrap(),
            ]);
            args.extend(extra);
        }
        f.cli(&args)
    };
    let without = review(&[]);
    assert_eq!(without.code, 2, "{}", without.stderr);
    assert!(
        without.stderr.contains("the session holds videos"),
        "{}",
        without.stderr
    );
    // The video's own settings bind its review, as the photo's bind a photo's.
    let other = review(&["--videos", "--video-quality", "balanced"]);
    assert_eq!(other.code, 1, "{}\n{}", other.stdout, other.stderr);
    assert!(
        other.stdout.contains("the models or settings differ"),
        "{}",
        other.stdout
    );
    assert!(sessions.join(&id).join("item-0/regions").exists());

    let reviewed = review(&["--videos"]);
    assert_eq!(reviewed.code, 0, "{}\n{}", reviewed.stdout, reviewed.stderr);
    assert!(
        reviewed.stdout.contains("published (reviewed)"),
        "{}",
        reviewed.stdout
    );
    assert_eq!(names(&f.out()), ["odd (2).mp4", "odd.mp4"]);
    assert_eq!(
        std::fs::read(f.out().join("odd.mp4")).unwrap(),
        b"someone else's"
    );
    assert!(names(&sessions).is_empty(), "{:?}", names(&sessions));

    let (_, source_frames) = frames(&source);
    let (_, output_frames) = frames(&f.out().join("odd (2).mp4"));
    assert_eq!(output_frames.len(), source_frames.len());
    for (source, output) in source_frames.iter().zip(&output_frames) {
        for (x, y) in [(150, 75), (299, 149)] {
            assert!(
                close(pixel(output, x, y), pixel(source, x, y), 24),
                "({x}, {y})"
            );
        }
        assert!(!close(pixel(source, 450, 280), [0, 0, 0], 30));
        assert!(close(pixel(output, 450, 280), [0, 0, 0], 6));
    }
}

/// A gap a manual track shows over in every frame is resolved by it; one it misses a frame of is
/// refused, and nothing is published.
#[test]
fn a_manual_track_resolves_a_gap_only_over_all_of_its_frames() {
    let f = Fixture::new();
    f.input(
        "odd.mp4",
        &fixtures().join("defects/P-5/odd-641x361-audio-longer.mp4"),
    );
    std::fs::create_dir_all(f.out()).unwrap();
    std::fs::write(f.out().join("odd.mp4"), b"someone else's").unwrap();
    let run = f.run_finding("100,50,200,100", &["--videos"]);
    assert_eq!(run.code, 3, "{}\n{}", run.stdout, run.stderr);
    let id = run
        .stdout
        .lines()
        .find_map(|l| l.strip_prefix("session\t"))
        .unwrap()
        .to_owned();
    // The stand-in detector leaves no gap; one is written into the stored evidence.
    let path = f.root.join("sessions").join(format!("{id}.json"));
    let mut stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    stored["items"][0]["evidence"] = serde_json::json!([{
        "kind": "unresolved_gap", "detection": "face", "tracks": [1],
        "first": 10, "last": 20, "resolution": "unresolved"
    }]);
    std::fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();

    let decisions = f.root.join("decisions.json");
    let review = |first: u64, last: u64| {
        std::fs::write(
            &decisions,
            format!(
                r#"{{"items": [{{"item": 0, "revision": 0, "action": "approve",
                    "rename_to": "odd (2).mp4",
                    "resolutions": [{{"evidence": 0, "resolution": "covered_by_manual_mask"}}],
                    "added_tracks": [{{"first": {first}, "last": {last}, "keyframes":
                        [{{"frame": {first}, "bbox": {{"x": 100, "y": 50, "width": 200, "height": 100}}}}]}}]}}]}}"#
            ),
        )
        .unwrap();
        let (ffmpeg, ffprobe) = (ffmpeg(), ffprobe());
        f.cli(&[
            "review",
            "--session",
            &id,
            "--decisions",
            decisions.to_str().unwrap(),
            "--tiles",
            "off",
            "--videos",
            "--ffmpeg",
            ffmpeg.to_str().unwrap(),
            "--ffprobe",
            ffprobe.to_str().unwrap(),
        ])
    };
    let short = review(11, 20);
    assert_eq!(short.code, 1, "{}\n{}", short.stdout, short.stderr);
    assert!(
        short.stdout.contains("refused")
            && short
                .stdout
                .contains("the added areas or tracks do not cover it"),
        "{}",
        short.stdout
    );
    assert_eq!(names(&f.out()), ["odd.mp4"]);
    let covering = review(10, 20);
    assert_eq!(covering.code, 0, "{}\n{}", covering.stdout, covering.stderr);
    assert_eq!(names(&f.out()), ["odd (2).mp4", "odd.mp4"]);
}

/// Records a run killed in its tracks (as a crash or a power cut leaves them) are removed by the
/// next run, but never those of a run still going.
#[test]
fn records_a_killed_run_left_go_with_the_next_run() {
    let f = Fixture::new();
    let long = f.root.join("in/long.mp4");
    let status = Command::new(ffmpeg())
        .args(["-v", "error", "-f", "lavfi"])
        .args(["-i", "testsrc2=s=640x360:r=30:d=120"])
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&long)
        .status()
        .unwrap();
    assert!(status.success());
    let sessions = f.root.join("sessions");
    let (out, ffmpeg, ffprobe) = (f.out(), ffmpeg(), ffprobe());
    let mut running = Command::new(env!("CARGO_BIN_EXE_cloakframe-cli"))
        .args(["run", "--output", out.to_str().unwrap(), "--videos"])
        .args(["--ffmpeg", ffmpeg.to_str().unwrap()])
        .args(["--ffprobe", ffprobe.to_str().unwrap()])
        .args(["--fixed-detection", "10,10,50,50", "--tiles", "off"])
        .arg("--sessions")
        .arg(&sessions)
        .arg(&long)
        .env("HOME", f.root.join("home"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let item_records = || {
        std::fs::read_dir(&sessions).ok().and_then(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.path().join("item-0"))
                .find(|p| p.exists())
        })
    };
    let started = std::time::Instant::now();
    let records = loop {
        if let Some(records) = item_records() {
            break records;
        }
        assert!(running.try_wait().unwrap().is_none(), "the run ended early");
        assert!(started.elapsed().as_secs() < 60, "no records appeared");
        std::thread::sleep(std::time::Duration::from_millis(20));
    };

    let photo = f.root.join("photo.jpg");
    std::fs::write(&photo, b"photo").unwrap();
    let other = |output: &str| {
        f.cli(&[
            "run",
            "--synthetic",
            "--review",
            "off",
            "--output",
            f.root.join(output).to_str().unwrap(),
            photo.to_str().unwrap(),
        ])
    };
    let beside = other("beside");
    assert_eq!(beside.code, 0, "{}", beside.stderr);
    assert!(
        running.try_wait().unwrap().is_none(),
        "the run ended before it could be killed; give it a longer video"
    );
    assert!(records.exists());
    running.kill().unwrap();
    running.wait().unwrap();
    assert!(records.exists());

    let after = other("after");
    assert_eq!(after.code, 0, "{}", after.stderr);
    assert!(!records.exists());
    assert!(names(&sessions).is_empty(), "{:?}", names(&sessions));
}

#[test]
fn videos_need_their_tools() {
    let f = Fixture::new();
    let usage = |args: &[&str], message: &str| {
        let run = f.run(args);
        assert_eq!(run.code, 2, "{args:?}: {}", run.stderr);
        assert!(run.stderr.contains(message), "{args:?}: {}", run.stderr);
    };
    usage(&["--mask", "0,0,8,8"], "the video options need --videos");
    usage(&["--videos", "--mask", "0,0,8"], "--mask is X,Y,W,H");
    usage(
        &["--fixed-detection", "0,0"],
        "--fixed-detection is X,Y,W,H",
    );
    usage(
        &["--videos", "--mask", "0,0,-8,8"],
        "width or height is negative",
    );
    let run = f.cli(&[
        "run",
        "--output",
        f.out().to_str().unwrap(),
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
