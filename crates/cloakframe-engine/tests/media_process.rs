//! The pinned FFmpeg and ffprobe (manifests/native.toml) under the process supervisor.

mod common;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::media::process::{ToolCommand, ToolError};
use common::{TestDir, required_env};

fn ffmpeg() -> PathBuf {
    required_env("CLOAKFRAME_FFMPEG")
}

fn ffprobe() -> PathBuf {
    required_env("CLOAKFRAME_FFPROBE")
}

/// Raw bgr24 frames from FFmpeg's test source; endless when `seconds` is `None`.
fn test_source(seconds: Option<u32>) -> ToolCommand {
    let source = match seconds {
        Some(s) => format!("testsrc=size=64x48:rate=10:duration={s}"),
        None => "testsrc=size=64x48:rate=10".into(),
    };
    let mut c = ToolCommand::ffmpeg(&ffmpeg());
    c.args([
        "-f", "lavfi", "-i", &source, "-f", "rawvideo", "-pix_fmt", "bgr24",
    ])
    .output_pipe();
    c
}

const FRAME: usize = 64 * 48 * 3;

#[test]
fn a_tool_runs_to_the_end_and_returns_its_output() {
    let out = test_source(Some(1)).output(&CancelToken::new()).unwrap();
    assert_eq!(out.len(), 10 * FRAME);
}

#[test]
fn failures_carry_the_end_of_stderr() {
    let dir = TestDir::new();
    let mut c = ToolCommand::ffmpeg(&ffmpeg());
    c.input_file(&dir.path().join("missing.mp4"))
        .args(["-f", "null"])
        .output_pipe();
    match c.output(&CancelToken::new()) {
        Err(ToolError::Failed { stderr, status, .. }) => {
            assert!(!status.success());
            assert!(stderr.contains("missing.mp4"), "{stderr}");
        }
        other => panic!("{other:?}"),
    }
}

fn assert_gone(pid: u32) {
    #[cfg(unix)]
    {
        let pid = rustix::process::Pid::from_raw(pid as i32).unwrap();
        assert!(rustix::process::test_kill_process(pid).is_err());
    }
    #[cfg(windows)]
    {
        let list = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .unwrap();
        let list = String::from_utf8_lossy(&list.stdout);
        assert!(!list.contains(&format!(" {pid} ")), "{list}");
    }
}

#[test]
fn cancelling_kills_a_running_tool() {
    let cancel = CancelToken::new();
    let mut running = test_source(None)
        .spawn(Stdio::null(), Stdio::piped(), &cancel)
        .unwrap();
    let pid = running.id();
    let mut stdout = running.take_stdout().unwrap();
    let mut frame = vec![0; FRAME];
    stdout.read_exact(&mut frame).unwrap();

    let cancelled = Instant::now();
    cancel.cancel();
    // The source is endless: only the kill ends this read.
    let (ended, read_ended) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = stdout.read_to_end(&mut Vec::new());
        let _ = ended.send(());
    });
    read_ended
        .recv_timeout(Duration::from_secs(10))
        .expect("the tool still writes after the cancel");
    assert!(matches!(running.wait(), Err(ToolError::Cancelled)));
    assert!(cancelled.elapsed() < Duration::from_secs(10));
    assert_gone(pid);
}

#[test]
fn dropping_a_running_tool_kills_it() {
    let mut running = test_source(None)
        .spawn(Stdio::null(), Stdio::piped(), &CancelToken::new())
        .unwrap();
    let pid = running.id();
    let mut frame = vec![0; FRAME];
    running
        .take_stdout()
        .unwrap()
        .read_exact(&mut frame)
        .unwrap();
    drop(running);
    assert_gone(pid);
}

#[test]
fn a_tool_already_finished_is_not_reported_as_cancelled() {
    let cancel = CancelToken::new();
    let mut running = test_source(Some(1))
        .spawn(Stdio::null(), Stdio::piped(), &cancel)
        .unwrap();
    let mut out = Vec::new();
    running
        .take_stdout()
        .unwrap()
        .read_to_end(&mut out)
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    cancel.cancel();
    running.wait().unwrap();
    assert_eq!(out.len(), 10 * FRAME);
}

#[test]
fn frames_go_through_stdin_and_come_back_on_stdout() {
    let frames: Vec<u8> = (0..3 * 4 * 4 * 3).map(|i| (i * 7 % 251) as u8).collect();
    let mut c = ToolCommand::ffmpeg(&ffmpeg());
    c.args([
        "-f", "rawvideo", "-pix_fmt", "bgr24", "-s", "4x4", "-r", "10",
    ])
    .input_pipe()
    .args(["-f", "rawvideo", "-pix_fmt", "bgr24"])
    .output_pipe();
    let mut running = c
        .spawn(Stdio::piped(), Stdio::piped(), &CancelToken::new())
        .unwrap();
    let mut stdin = running.take_stdin().unwrap();
    let sent = frames.clone();
    let writer = std::thread::spawn(move || stdin.write_all(&sent));
    let mut out = Vec::new();
    running
        .take_stdout()
        .unwrap()
        .read_to_end(&mut out)
        .unwrap();
    writer.join().unwrap().unwrap();
    running.wait().unwrap();
    assert_eq!(out, frames);
}

fn write_tone(path: &Path) {
    let mut c = ToolCommand::ffmpeg(&ffmpeg());
    c.args(["-f", "lavfi", "-i", "sine=frequency=440:duration=0.5"])
        .output_file(path);
    c.output(&CancelToken::new()).unwrap();
}

/// A playlist inside a local file must not make the tool open anything but local files. FFmpeg's
/// own HLS default already refuses network segments of a local playlist but allows `data:`, so a
/// `data:` segment shows that this whitelist is the one in force.
#[test]
fn inputs_open_only_local_files() {
    let dir = TestDir::new();
    let local = dir.path().join("local.wav");
    write_tone(&local);
    let playlist = dir.path().join("list.m3u8");
    std::fs::write(
        &playlist,
        "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\ndata:,segment.ts\n#EXT-X-ENDLIST\n",
    )
    .unwrap();
    let mut c = ToolCommand::ffmpeg(&ffmpeg());
    c.input_file(&playlist).args(["-f", "null"]).output_pipe();
    match c.output(&CancelToken::new()) {
        Err(ToolError::Failed { stderr, .. }) => {
            assert!(
                stderr.contains("Protocol 'data' not on whitelist 'file'"),
                "{stderr}"
            );
        }
        other => panic!("{other:?}"),
    }

    let mut c = ToolCommand::ffmpeg(&ffmpeg());
    c.input_file(&local).args(["-f", "null"]).output_pipe();
    c.output(&CancelToken::new()).unwrap();
}

#[test]
fn ffprobe_reports_on_a_local_file() {
    let dir = TestDir::new();
    let tone = dir.path().join("tone.wav");
    write_tone(&tone);
    let mut c = ToolCommand::ffprobe(&ffprobe());
    c.args([
        "-show_entries",
        "stream=codec_type,sample_rate",
        "-of",
        "json",
    ])
    .input_file(&tone);
    let report = String::from_utf8(c.output(&CancelToken::new()).unwrap()).unwrap();
    assert!(report.contains("\"codec_type\": \"audio\""), "{report}");
}
