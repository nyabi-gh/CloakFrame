//! FFmpeg and ffprobe as supervised child processes. They are not a security boundary (plan 0.1):
//! inputs are limited to the `file` and `pipe` protocols, and cancelling kills the whole process
//! tree only so nothing keeps running or writing after the item ends.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::cancel::CancelToken;

/// How much of a tool's stderr is kept for the error message.
const STDERR_TAIL: usize = 8 * 1024;
const CANCEL_POLL: Duration = Duration::from_millis(50);
const REAP_POLL_MAX: Duration = Duration::from_millis(50);

/// A command line for one of the media tools.
pub struct ToolCommand {
    command: Command,
    tool: String,
}

impl ToolCommand {
    pub fn ffmpeg(program: &Path) -> Self {
        let mut c = Self::new(program);
        c.command.arg("-nostdin");
        c
    }

    pub fn ffprobe(program: &Path) -> Self {
        Self::new(program)
    }

    fn new(program: &Path) -> Self {
        let mut command = Command::new(program);
        command.args(["-hide_banner", "-loglevel", "error"]);
        let tool = program.file_stem().map_or_else(
            || program.display().to_string(),
            |s| s.to_string_lossy().into(),
        );
        Self { command, tool }
    }

    pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Self {
        self.command.arg(arg);
        self
    }

    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.command.args(args);
        self
    }

    /// A local file input. The `file:` prefix keeps a name such as `concat:a|b` from being read
    /// as another protocol, and the whitelist stops a playlist or demuxer inside the file from
    /// opening anything but local files.
    pub fn input_file(&mut self, path: &Path) -> &mut Self {
        self.args(["-protocol_whitelist", "file", "-i"])
            .arg(file_url(path))
    }

    /// Input from this process through the tool's stdin.
    pub fn input_pipe(&mut self) -> &mut Self {
        self.args(["-protocol_whitelist", "pipe", "-i", "pipe:0"])
    }

    pub fn output_file(&mut self, path: &Path) -> &mut Self {
        self.arg(file_url(path))
    }

    pub fn output_pipe(&mut self) -> &mut Self {
        self.arg("pipe:1")
    }

    /// Starts the tool with stderr kept and the given stdin and stdout.
    pub fn spawn(
        mut self,
        stdin: Stdio,
        stdout: Stdio,
        cancel: &CancelToken,
    ) -> Result<Supervised, ToolError> {
        self.command
            .stdin(stdin)
            .stdout(stdout)
            .stderr(Stdio::piped());
        os::own_process_group(&mut self.command);
        let mut child = self.command.spawn().map_err(|source| ToolError::Spawn {
            tool: self.tool.clone(),
            source,
        })?;
        let tree = match os::Tree::adopt(&child) {
            Ok(tree) => tree,
            Err(source) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ToolError::Spawn {
                    tool: self.tool,
                    source,
                });
            }
        };
        let stderr = child.stderr.take().map(|mut err| {
            thread::Builder::new()
                .name("cloakframe-tool-stderr".into())
                .spawn(move || read_tail(&mut err))
        });
        let stderr = match stderr.transpose() {
            Ok(stderr) => stderr,
            Err(source) => {
                tree.kill();
                let _ = child.wait();
                return Err(ToolError::Spawn {
                    tool: self.tool,
                    source,
                });
            }
        };
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                tree: Some(tree),
                killed: false,
                finished: false,
            }),
            wake: Condvar::new(),
        });
        let watcher = {
            let shared = shared.clone();
            let cancel = cancel.clone();
            thread::Builder::new()
                .name("cloakframe-tool-cancel".into())
                .spawn(move || watch(&shared, &cancel))
        };
        let mut supervised = Supervised {
            tool: self.tool,
            child: Some(child),
            shared,
            watcher: None,
            stderr,
        };
        match watcher {
            Ok(watcher) => supervised.watcher = Some(watcher),
            Err(source) => {
                return Err(ToolError::Spawn {
                    tool: supervised.tool.clone(),
                    source,
                });
            }
        }
        Ok(supervised)
    }

    /// Runs the tool to the end and returns its stdout (ffprobe's report).
    pub fn output(self, cancel: &CancelToken) -> Result<Vec<u8>, ToolError> {
        let mut running = self.spawn(Stdio::null(), Stdio::piped(), cancel)?;
        let mut out = Vec::new();
        let read = running
            .take_stdout()
            .expect("stdout is piped")
            .read_to_end(&mut out);
        running.wait()?;
        read.map_err(ToolError::Io)?;
        Ok(out)
    }
}

fn file_url(path: &Path) -> OsString {
    let mut url = OsString::from("file:");
    url.push(path);
    url
}

fn read_tail(stream: &mut impl Read) -> String {
    let mut tail = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                tail.extend_from_slice(&buf[..n]);
                if tail.len() > STDERR_TAIL {
                    tail.drain(..tail.len() - STDERR_TAIL);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&tail).trim_end().to_owned()
}

#[derive(Debug)]
pub enum ToolError {
    Spawn {
        tool: String,
        source: io::Error,
    },
    Io(io::Error),
    Failed {
        tool: String,
        status: ExitStatus,
        stderr: String,
    },
    Cancelled,
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { tool, source } => write!(f, "cannot start {tool}: {source}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Failed {
                tool,
                status,
                stderr,
            } if stderr.is_empty() => write!(f, "{tool} failed ({status})"),
            Self::Failed {
                tool,
                status,
                stderr,
            } => write!(f, "{tool} failed ({status}): {stderr}"),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for ToolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } | Self::Io(source) => Some(source),
            _ => None,
        }
    }
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

/// `tree` is dropped once the child is reaped. Killing and reaping happen under this lock, so a
/// kill never reaches a process group id the OS has already handed to someone else.
struct State {
    tree: Option<os::Tree>,
    killed: bool,
    finished: bool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn kill(&self) {
        let mut state = self.lock();
        if let Some(tree) = &state.tree {
            tree.kill();
            state.killed = true;
        }
    }
}

fn watch(shared: &Shared, cancel: &CancelToken) {
    let mut state = shared.lock();
    while !state.finished {
        if !state.killed && cancel.is_cancelled() {
            if let Some(tree) = &state.tree {
                tree.kill();
            }
            state.killed = true;
        }
        state = shared
            .wake
            .wait_timeout(state, CANCEL_POLL)
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
}

/// A running tool. Dropping it kills the process tree and reaps it.
pub struct Supervised {
    tool: String,
    child: Option<Child>,
    shared: Arc<Shared>,
    watcher: Option<JoinHandle<()>>,
    stderr: Option<JoinHandle<String>>,
}

impl Supervised {
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.as_mut().and_then(|c| c.stdin.take())
    }

    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.as_mut().and_then(|c| c.stdout.take())
    }

    pub fn id(&self) -> u32 {
        self.child.as_ref().expect("not yet reaped").id()
    }

    /// Kills the process tree now; [`Self::wait`] then reports [`ToolError::Cancelled`].
    pub fn kill(&self) {
        self.shared.kill();
    }

    /// Waits for the tool to end. Close or drop its stdin first if it reads from this process.
    pub fn wait(mut self) -> Result<(), ToolError> {
        let status = self.reap().map_err(ToolError::Io)?;
        let stderr = self.finish();
        if self.shared.lock().killed {
            return Err(ToolError::Cancelled);
        }
        if !status.success() {
            return Err(ToolError::Failed {
                tool: self.tool.clone(),
                status,
                stderr,
            });
        }
        Ok(())
    }

    /// Polls rather than blocks, because a blocking wait would reap without the lock that the
    /// cancel watcher kills under.
    fn reap(&mut self) -> io::Result<ExitStatus> {
        let mut child = self.child.take().expect("reaped once");
        drop(child.stdin.take());
        let mut pause = Duration::from_millis(1);
        loop {
            let mut state = self.shared.lock();
            match child.try_wait() {
                Ok(Some(status)) => {
                    state.tree = None;
                    return Ok(status);
                }
                Ok(None) => {}
                Err(e) => {
                    drop(state);
                    self.shared.kill();
                    let _ = child.wait();
                    self.shared.lock().tree = None;
                    return Err(e);
                }
            }
            drop(state);
            thread::sleep(pause);
            pause = (pause * 2).min(REAP_POLL_MAX);
        }
    }

    fn finish(&mut self) -> String {
        self.shared.lock().finished = true;
        self.shared.wake.notify_all();
        if let Some(watcher) = self.watcher.take() {
            let _ = watcher.join();
        }
        self.stderr
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default()
    }
}

impl Drop for Supervised {
    fn drop(&mut self) {
        if self.child.is_some() {
            self.shared.kill();
            let _ = self.reap();
        }
        self.finish();
    }
}

#[cfg(unix)]
mod os {
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};

    use rustix::process::{Pid, Signal, kill_process_group};

    /// Its own group, so a terminal Ctrl+C reaches only the CLI, which then ends the tool.
    pub fn own_process_group(command: &mut Command) {
        command.process_group(0);
    }

    pub struct Tree(Pid);

    impl Tree {
        pub fn adopt(child: &Child) -> std::io::Result<Self> {
            let pid = i32::try_from(child.id())
                .ok()
                .and_then(Pid::from_raw)
                .ok_or_else(|| std::io::Error::other("invalid child pid"))?;
            Ok(Self(pid))
        }

        pub fn kill(&self) {
            // ESRCH: everything in the group has already exited.
            let _ = kill_process_group(self.0, Signal::KILL);
        }
    }
}

#[cfg(windows)]
mod os {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command};

    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;

    /// A new process group does not receive the console's Ctrl+C, which then reaches only the CLI.
    pub fn own_process_group(command: &mut Command) {
        command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }

    /// A job object holding the tool and anything it starts. Closing the last handle kills them,
    /// which also covers this process ending without cleaning up.
    pub struct Tree(OwnedHandle);

    impl Tree {
        #[allow(unsafe_code)]
        pub fn adopt(child: &Child) -> std::io::Result<Self> {
            // SAFETY: no security attributes and no name; the result is checked before use.
            let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if raw.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            // SAFETY: `raw` is a new job handle that nothing else owns.
            let job = unsafe { OwnedHandle::from_raw_handle(raw) };
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: `limits` is the structure this information class takes, with its size.
            let set = unsafe {
                SetInformationJobObject(
                    job.as_raw_handle(),
                    JobObjectExtendedLimitInformation,
                    std::ptr::from_ref(&limits).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if set == 0 {
                return Err(std::io::Error::last_os_error());
            }
            // SAFETY: both handles are open for the duration of the call.
            if unsafe { AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) } == 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(Self(job))
        }

        #[allow(unsafe_code)]
        pub fn kill(&self) {
            // SAFETY: the job handle is open while `self` lives.
            let _ = unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(program: &str, args: &[&str]) -> ToolCommand {
        let mut command = Command::new(program);
        command.args(args);
        #[cfg(windows)]
        command.env_remove("PSModulePath");
        ToolCommand {
            command,
            tool: program.into(),
        }
    }

    /// The tool starts a grandchild, prints its pid and waits for it.
    #[cfg(unix)]
    fn tool_with_grandchild() -> ToolCommand {
        tool("sh", &["-c", "sleep 60 & echo $!; wait"])
    }

    #[cfg(windows)]
    fn tool_with_grandchild() -> ToolCommand {
        tool(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "$p = Start-Process -FilePath ping -ArgumentList '-n','60','127.0.0.1' \
                 -PassThru -NoNewWindow; $p.Id; [Console]::Out.Flush(); $p.WaitForExit()",
            ],
        )
    }

    #[cfg(unix)]
    fn is_running(pid: u32) -> bool {
        let pid = rustix::process::Pid::from_raw(pid as i32).unwrap();
        rustix::process::test_kill_process(pid).is_ok()
    }

    #[cfg(windows)]
    fn is_running(pid: u32) -> bool {
        let list = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&list.stdout).contains(&format!(" {pid} "))
    }

    fn read_pid(running: &mut Supervised) -> u32 {
        let mut stdout = running.take_stdout().unwrap();
        let mut line = Vec::new();
        let mut byte = [0u8];
        while stdout.read(&mut byte).unwrap() == 1 && byte[0] != b'\n' {
            line.push(byte[0]);
        }
        String::from_utf8(line).unwrap().trim().parse().unwrap()
    }

    /// The grandchild's parent is gone once the tree is killed, so it is reaped by the system
    /// shortly after; until then it may still show as a zombie.
    fn assert_ends(pid: u32) {
        let start = std::time::Instant::now();
        while is_running(pid) {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "process {pid} still runs"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn cancelling_kills_the_whole_tree() {
        let cancel = CancelToken::new();
        let mut running = tool_with_grandchild()
            .spawn(Stdio::null(), Stdio::piped(), &cancel)
            .unwrap();
        let grandchild = read_pid(&mut running);
        assert!(is_running(grandchild));
        let cancelled = std::time::Instant::now();
        cancel.cancel();
        assert!(matches!(running.wait(), Err(ToolError::Cancelled)));
        // A surviving grandchild holds stderr open, so the wait itself would last its lifetime.
        assert!(cancelled.elapsed() < Duration::from_secs(10));
        assert_ends(grandchild);
    }

    #[test]
    fn dropping_kills_the_whole_tree() {
        let mut running = tool_with_grandchild()
            .spawn(Stdio::null(), Stdio::piped(), &CancelToken::new())
            .unwrap();
        let grandchild = read_pid(&mut running);
        let dropped = std::time::Instant::now();
        drop(running);
        assert!(dropped.elapsed() < Duration::from_secs(10));
        assert_ends(grandchild);
    }

    #[test]
    fn a_missing_program_cannot_start() {
        let err = tool("cloakframe-no-such-tool", &[])
            .spawn(Stdio::null(), Stdio::null(), &CancelToken::new())
            .err()
            .unwrap();
        assert!(matches!(err, ToolError::Spawn { .. }), "{err}");
    }

    #[test]
    fn stderr_keeps_only_its_end() {
        let long = "x".repeat(3 * STDERR_TAIL) + "END";
        let tail = read_tail(&mut long.as_bytes());
        assert_eq!(tail.len(), STDERR_TAIL);
        assert!(tail.ends_with("END"));
    }
}
