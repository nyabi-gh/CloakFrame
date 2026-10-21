//! The M1 exit criteria end to end through `cloakframe-cli` with the synthetic processor: normal,
//! failing and review paths, sources and existing outputs left unchanged, no temp file under a
//! final name, and P-1 (nothing with an unresolved problem is published unless explicitly
//! exported with warnings).
#![cfg(feature = "synthetic")]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::TestDir;

struct Fixture {
    root: PathBuf,
    _dir: TestDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = TestDir::new();
        let root = dir.mkdir("in").parent().unwrap().to_path_buf();
        Self { root, _dir: dir }
    }

    fn input(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join("in").join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn out(&self) -> PathBuf {
        self.root.join("out")
    }

    fn cli(&self, args: &[&str]) -> (i32, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_cloakframe-cli"))
            .args(args)
            .arg("--sessions")
            .arg(self.root.join("sessions"))
            .arg("--synthetic")
            .output()
            .unwrap();
        (
            output.status.code().unwrap(),
            String::from_utf8(output.stdout).unwrap(),
        )
    }

    /// The exit code and what went to stderr.
    fn cli_stderr(&self, args: &[&str]) -> (i32, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_cloakframe-cli"))
            .args(args)
            .arg("--sessions")
            .arg(self.root.join("sessions"))
            .arg("--synthetic")
            .output()
            .unwrap();
        (
            output.status.code().unwrap(),
            String::from_utf8(output.stderr).unwrap(),
        )
    }

    fn run(&self, review: &str) -> (i32, String) {
        let input = self.root.join("in");
        let out = self.out();
        self.cli(&[
            "run",
            "--output",
            out.to_str().unwrap(),
            "--review",
            review,
            "--recursive",
            input.to_str().unwrap(),
        ])
    }

    fn review(&self, session: &str, decisions: &str) -> (i32, String) {
        let path = self.root.join("decisions.json");
        std::fs::write(&path, decisions).unwrap();
        self.cli(&[
            "review",
            "--session",
            session,
            "--decisions",
            path.to_str().unwrap(),
        ])
    }

    fn sessions(&self) -> usize {
        std::fs::read_dir(self.root.join("sessions")).map_or(0, |d| d.count())
    }
}

fn session_id(stdout: &str) -> String {
    stdout
        .lines()
        .find_map(|l| l.strip_prefix("session\t"))
        .expect("a session was saved")
        .to_owned()
}

fn files(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(d) = pending.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                pending.push(e.path());
            } else {
                let relative = e.path().strip_prefix(dir).unwrap().to_owned();
                found.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    found.sort();
    found
}

fn reversed(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().rev().copied().collect()
}

#[test]
fn automatic_run_publishes_every_clean_item_and_leaves_sources_alone() {
    let f = Fixture::new();
    let a = f.input("a.jpg", b"first image");
    let b = f.input("nested/b.png", b"second image");
    let (code, stdout) = f.run("off");
    assert_eq!(code, 0, "{stdout}");
    assert_eq!(files(&f.out()), ["a.jpg", "nested/b.png"]);
    assert_eq!(
        std::fs::read(f.out().join("a.jpg")).unwrap(),
        reversed(b"first image")
    );
    assert_eq!(std::fs::read(&a).unwrap(), b"first image");
    assert_eq!(std::fs::read(&b).unwrap(), b"second image");
    assert_eq!(f.sessions(), 0);
    assert!(stdout.contains("published (automatic, unreviewed)"));
}

#[test]
fn a_failing_item_fails_alone() {
    let f = Fixture::new();
    f.input("ok.jpg", b"ok");
    f.input("fail.jpg", b"broken");
    let (code, stdout) = f.run("off");
    assert_eq!(code, 1, "{stdout}");
    assert_eq!(files(&f.out()), ["ok.jpg"]);
    assert!(stdout.contains("failed") && stdout.contains("synthetic decode failure"));
}

#[test]
fn an_existing_output_is_kept_and_the_item_waits_for_a_new_name() {
    let f = Fixture::new();
    f.input("a.jpg", b"new");
    std::fs::create_dir_all(f.out()).unwrap();
    std::fs::write(f.out().join("a.jpg"), b"user's file").unwrap();

    let (code, stdout) = f.run("off");
    assert_eq!(code, 3, "{stdout}");
    assert_eq!(
        std::fs::read(f.out().join("a.jpg")).unwrap(),
        b"user's file"
    );
    assert_eq!(files(&f.out()), ["a.jpg"]);
    let session = session_id(&stdout);

    let (code, stdout) = f.review(
        &session,
        r#"{"items": [{"item": 0, "revision": 0, "action": "approve"}]}"#,
    );
    assert_eq!(code, 1, "{stdout}");
    assert!(stdout.contains("a (2).jpg"), "{stdout}");

    let (code, stdout) = f.review(
        &session,
        r#"{"items": [{"item": 0, "revision": 0, "action": "approve", "rename_to": "a (2).jpg"}]}"#,
    );
    assert_eq!(code, 0, "{stdout}");
    assert_eq!(files(&f.out()), ["a (2).jpg", "a.jpg"]);
    assert_eq!(
        std::fs::read(f.out().join("a.jpg")).unwrap(),
        b"user's file"
    );
    assert_eq!(f.sessions(), 0);
}

// P-1: unresolved problems hold the item back even with review off.
#[test]
fn unresolved_problems_publish_nothing_until_resolved_or_explicitly_exported() {
    let f = Fixture::new();
    f.input("clip-gap.jpg", b"gap");
    f.input("crowd-capped.jpg", b"capped");
    f.input("clean.jpg", b"clean");

    let (code, stdout) = f.run("off");
    assert_eq!(code, 3, "{stdout}");
    assert_eq!(files(&f.out()), ["clean.jpg"]);
    let session = session_id(&stdout);
    let stored =
        std::fs::read_to_string(f.root.join("sessions").join(format!("{session}.json"))).unwrap();
    assert!(stored.contains("\"unresolved_gap\"") && stored.contains("\"capped_candidates\""));

    // Approving without resolving is refused, and nothing is written.
    let (code, stdout) = f.review(
        &session,
        r#"{"items": [{"item": 1, "revision": 0, "action": "approve"}]}"#,
    );
    assert_eq!(code, 1, "{stdout}");
    assert!(stdout.contains("unresolved detection problems remain"));
    assert_eq!(files(&f.out()), ["clean.jpg"]);

    // Dismissing the gap publishes that item as reviewed; the capped one is exported with
    // warnings, which is published but not a normal completion.
    let (code, stdout) = f.review(
        &session,
        r#"{"items": [
            {"item": 1, "revision": 0, "action": "approve",
             "resolutions": [{"evidence": 0, "resolution": "dismissed"}]},
            {"item": 2, "revision": 0, "action": "export_with_warnings"}
        ]}"#,
    );
    assert_eq!(code, 0, "{stdout}");
    assert!(stdout.contains("published (reviewed)"));
    assert!(stdout.contains("exported with warnings"));
    assert_eq!(
        files(&f.out()),
        ["clean.jpg", "clip-gap.jpg", "crowd-capped.jpg"]
    );
    assert_eq!(f.sessions(), 0);
}

#[test]
fn review_required_publishes_nothing_before_review() {
    let f = Fixture::new();
    f.input("a.jpg", b"a");
    let (code, stdout) = f.run("required");
    assert_eq!(code, 3, "{stdout}");
    assert!(files(&f.out()).is_empty());
    assert_eq!(f.sessions(), 1);
}

#[test]
fn stale_or_changed_reviews_are_refused() {
    let f = Fixture::new();
    let a = f.input("a.jpg", b"a");
    f.input("b.jpg", b"b");
    let (_, stdout) = f.run("required");
    let session = session_id(&stdout);

    let (code, stdout) = f.review(
        &session,
        r#"{"items": [{"item": 0, "revision": 3, "action": "approve"}]}"#,
    );
    assert_eq!(code, 1);
    assert!(stdout.contains("older revision"), "{stdout}");

    std::fs::write(&a, b"edited after analysis").unwrap();
    let (code, stdout) = f.review(
        &session,
        r#"{"items": [{"item": 0, "revision": 0, "action": "approve"}]}"#,
    );
    assert_eq!(code, 1);
    assert!(stdout.contains("source file changed"), "{stdout}");

    let (code, stdout) = f.review(
        &session,
        r#"{"items": [{"item": 1, "revision": 0, "action": "approve"}]}"#,
    );
    assert_eq!(code, 3, "{stdout}");
    assert_eq!(files(&f.out()), ["b.jpg"]);
    assert_eq!(f.sessions(), 1);
}

#[test]
fn the_output_folder_inside_the_input_is_not_scanned() {
    let f = Fixture::new();
    f.input("a.jpg", b"a");
    let out = f.root.join("in/out");
    let input = f.root.join("in");
    for _ in 0..2 {
        let (code, stdout) = f.cli(&[
            "run",
            "--output",
            out.to_str().unwrap(),
            "--review",
            "off",
            input.to_str().unwrap(),
        ]);
        assert!(code == 0 || code == 3, "{stdout}");
    }
    assert_eq!(files(&out), ["a.jpg"]);
}

/// A failure while running is not a usage error: exit 1 with the reason and no usage text.
#[test]
fn runtime_errors_exit_with_one() {
    let f = Fixture::new();
    f.input("a-gap.jpg", b"gap");
    std::fs::write(f.root.join("sessions"), b"not a folder").unwrap();
    let (code, stderr) = f.cli_stderr(&[
        "run",
        "--output",
        f.out().to_str().unwrap(),
        "--review",
        "off",
        f.root.join("in").to_str().unwrap(),
    ]);
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.contains("cannot save the review session"),
        "{stderr}"
    );
    assert!(!stderr.contains("usage:"), "{stderr}");
}

#[test]
fn usage_errors_exit_with_two() {
    let f = Fixture::new();
    assert_eq!(f.cli(&["run"]).0, 2);
    assert_eq!(f.cli(&["frobnicate"]).0, 2);
    assert_eq!(f.cli(&["review", "--session", "../x"]).0, 2);
}

#[cfg(unix)]
mod interrupt {
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    use rustix::process::{Pid, Signal, kill_process};

    use super::Fixture;

    fn start(f: &Fixture) -> Child {
        Command::new(env!("CARGO_BIN_EXE_cloakframe-cli"))
            .args(["run", "--output"])
            .arg(f.out())
            .args(["--review", "off", "--workers", "1", "--sessions"])
            .arg(f.root.join("sessions"))
            .arg("--synthetic")
            .arg(f.root.join("in"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    fn interrupt(child: &Child) {
        let pid = Pid::from_raw(child.id() as i32).unwrap();
        kill_process(pid, Signal::INT).unwrap();
    }

    /// Waits until the item is rendering, which is when its temp output exists.
    fn wait_for_temp_output(f: &Fixture) {
        let start = Instant::now();
        loop {
            let names: Vec<_> = std::fs::read_dir(f.out())
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            if names.iter().any(|n| n.starts_with(".cloakframe-tmp-")) {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(30), "{names:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn exit_within(child: &mut Child, limit: Duration) -> Option<i32> {
        let start = Instant::now();
        while start.elapsed() < limit {
            if let Some(status) = child.try_wait().unwrap() {
                return Some(status.code().unwrap());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }

    #[test]
    fn ctrl_c_cancels_the_run_and_publishes_nothing() {
        let f = Fixture::new();
        f.input("a-slow.jpg", b"slow");
        f.input("b.jpg", b"later");
        let mut child = start(&f);
        wait_for_temp_output(&f);
        interrupt(&child);
        let code = exit_within(&mut child, Duration::from_secs(10));
        if code.is_none() {
            let _ = child.kill();
        }
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(code, Some(130), "{stdout}");
        assert!(stdout.contains("cancelled"), "{stdout}");
        assert_eq!(super::files(&f.out()), Vec::<String>::new());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("press Ctrl+C again"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// `nohup` and background jobs of scripts start the CLI with hangups or interrupts ignored.
    #[test]
    fn signals_ignored_at_start_stay_ignored() {
        let f = Fixture::new();
        f.input("a-slow.jpg", b"slow");
        let mut child = Command::new("nohup")
            .arg(env!("CARGO_BIN_EXE_cloakframe-cli"))
            .args(["run", "--output"])
            .arg(f.out())
            .args(["--review", "off", "--workers", "1", "--sessions"])
            .arg(f.root.join("sessions"))
            .arg("--synthetic")
            .arg(f.root.join("in"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        wait_for_temp_output(&f);
        let pid = Pid::from_raw(child.id() as i32).unwrap();
        kill_process(pid, Signal::HUP).unwrap();
        let after_hangup = exit_within(&mut child, Duration::from_millis(1500));
        if after_hangup.is_none() {
            interrupt(&child);
        }
        let code = exit_within(&mut child, Duration::from_secs(10));
        if code.is_none() {
            let _ = child.kill();
        }
        let output = child.wait_with_output().unwrap();
        assert_eq!(
            after_hangup,
            None,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(code, Some(130));
    }

    #[test]
    fn a_second_ctrl_c_exits_at_once() {
        let f = Fixture::new();
        f.input("a-stuck.jpg", b"stuck");
        let mut child = start(&f);
        wait_for_temp_output(&f);
        interrupt(&child);
        assert_eq!(exit_within(&mut child, Duration::from_millis(500)), None);
        interrupt(&child);
        let code = exit_within(&mut child, Duration::from_secs(10));
        if code.is_none() {
            let _ = child.kill();
        }
        assert_eq!(code, Some(130));
    }
}
