//! Showing a file in the system's file manager, or opening it in its default app.

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

/// Starts `command` and reaps it on a thread of its own, so it does not linger as a zombie.
/// Only starting it can fail here: the file managers exit with codes that say nothing (Explorer
/// returns 1 when it worked).
fn launch(mut command: Command) -> io::Result<()> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    std::thread::Builder::new()
        .name("cloakframe-shell".into())
        .spawn(move || child.wait())?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn reveal(path: &Path) -> io::Result<()> {
    let mut command = Command::new("/usr/bin/open");
    command.arg("-R").arg(path);
    launch(command)
}

#[cfg(target_os = "macos")]
pub fn open(path: &Path) -> io::Result<()> {
    let mut command = Command::new("/usr/bin/open");
    command.arg(path);
    launch(command)
}

#[cfg(target_os = "windows")]
fn explorer() -> Command {
    let windows = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    Command::new(Path::new(&windows).join("explorer.exe"))
}

#[cfg(target_os = "windows")]
pub fn reveal(path: &Path) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    let mut command = explorer();
    // Explorer reads "/select," and the quoted path as one argument, which the standard quoting
    // would split. Windows paths cannot hold a quote.
    command.raw_arg(format!("/select,\"{}\"", path.display()));
    launch(command)
}

#[cfg(target_os = "windows")]
pub fn open(path: &Path) -> io::Result<()> {
    let mut command = explorer();
    command.arg(path);
    launch(command)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn reveal(path: &Path) -> io::Result<()> {
    // The freedesktop file manager interface selects the file; without a file manager that has
    // it, the folder opens.
    if show_items(path).is_ok() {
        return Ok(());
    }
    open(path.parent().unwrap_or(path))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn open(path: &Path) -> io::Result<()> {
    let mut command = Command::new("xdg-open");
    command.arg(path);
    launch(command)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn show_items(path: &Path) -> zbus::Result<()> {
    let connection = zbus::blocking::Connection::session()?;
    connection.call_method(
        Some("org.freedesktop.FileManager1"),
        "/org/freedesktop/FileManager1",
        Some("org.freedesktop.FileManager1"),
        "ShowItems",
        &(vec![file_uri(path)], ""),
    )?;
    Ok(())
}

/// A `file://` URI with every byte outside RFC 3986's unreserved set and `/` percent-encoded.
#[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
fn file_uri(path: &Path) -> String {
    let bytes = path.as_os_str().as_encoded_bytes();
    let mut uri = String::from("file://");
    for &b in bytes {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_escape_everything_but_unreserved_bytes() {
        assert_eq!(
            file_uri(Path::new("/home/a b/髙橋 #1.jpg")),
            "file:///home/a%20b/%E9%AB%99%E6%A9%8B%20%231.jpg"
        );
    }
}
