use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::hash::{BuildHasher, RandomState};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use cloakframe_core::item::PublishPermit;

pub const TEMP_PREFIX: &str = ".cloakframe-tmp-";

/// Temp files older than this in a folder the run writes to are leftovers of a crashed run.
pub const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// An output being written next to its destination under a hidden temporary name. It becomes
/// visible under the final name only through [`TempOutput::publish`], which needs the item's
/// publish permit and never replaces an existing file. Dropping it unpublished removes it.
///
/// The file and any folders are created with default permissions, so they follow the umask and
/// the folder's inherited ACL (D4), and no copy of the source is made anywhere else (D12).
#[derive(Debug)]
pub struct TempOutput {
    file: Option<File>,
    temp: PathBuf,
    destination: PathBuf,
    published: bool,
}

impl TempOutput {
    pub fn create(destination: &Path) -> io::Result<Self> {
        let dir = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(dir)?;
        let extension = destination.extension().map(|e| {
            let mut dotted = OsString::from(".");
            dotted.push(e);
            dotted
        });
        loop {
            let mut candidate = OsString::from(TEMP_PREFIX);
            candidate.push(format!(
                "{:016x}",
                RandomState::new().hash_one(SystemTime::now())
            ));
            if let Some(extension) = &extension {
                candidate.push(extension);
            }
            let temp = dir.join(&candidate);
            match OpenOptions::new().write(true).create_new(true).open(&temp) {
                Ok(file) => {
                    return Ok(Self {
                        file: Some(file),
                        temp,
                        destination: destination.to_path_buf(),
                        published: false,
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
    }

    pub fn file(&mut self) -> &mut File {
        self.file.as_mut().expect("open until published")
    }

    /// For writers that need a path, such as FFmpeg. The extension matches the destination's.
    pub fn temp_path(&self) -> &Path {
        &self.temp
    }

    pub fn destination(&self) -> &Path {
        &self.destination
    }

    /// Flushes the file to disk and moves it to the destination. Taking the permit means no
    /// output becomes visible for an item that was not cleared for publishing (P-1). Fails with
    /// `io::ErrorKind::AlreadyExists` if anything has appeared under that name.
    pub fn publish(mut self, _permit: &PublishPermit) -> io::Result<PathBuf> {
        self.file.take();
        // Reopened so that data written through `temp_path` by another process is flushed too.
        File::options().write(true).open(&self.temp)?.sync_all()?;
        rename_no_replace(&self.temp, &self.destination)?;
        self.published = true;
        #[cfg(unix)]
        if let Some(dir) = self.destination.parent() {
            File::open(dir)?.sync_all()?;
        }
        Ok(self.destination.clone())
    }
}

impl Drop for TempOutput {
    fn drop(&mut self) {
        if !self.published {
            self.file.take();
            let _ = fs::remove_file(&self.temp);
        }
    }
}

/// Removes temp files left in `dir` by runs that ended without cleaning up. Only files with the
/// temp prefix that are older than [`STALE_AFTER`] are touched, so a run still writing in the
/// same folder is safe.
pub fn remove_stale_temp_files(dir: &Path, now: SystemTime) -> io::Result<usize> {
    let mut removed = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_name().to_string_lossy().starts_with(TEMP_PREFIX) {
            continue;
        }
        let metadata = entry.metadata()?;
        let age = metadata
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok());
        if metadata.is_file() && age.is_some_and(|age| age > STALE_AFTER) {
            fs::remove_file(entry.path())?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(unix)]
fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    use rustix::io::Errno;

    match renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE) {
        Ok(()) => return Ok(()),
        // The filesystem or kernel lacks an exclusive rename (FAT, exFAT, some network volumes).
        // NOTSUP and OPNOTSUPP are the same value on Linux but not on macOS.
        Err(e) if [Errno::INVAL, Errno::NOSYS, Errno::NOTSUP, Errno::OPNOTSUPP].contains(&e) => {}
        Err(e) => return Err(e.into()),
    }
    match fs::hard_link(from, to) {
        Ok(()) => return fs::remove_file(from),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Err(e),
        Err(_) => {}
    }
    rename_if_absent(from, to)
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};

    let wide = |p: &Path| -> Vec<u16> { p.as_os_str().encode_wide().chain([0]).collect() };
    let (from_w, to_w) = (wide(from), wide(to));
    // SAFETY: both buffers are NUL-terminated UTF-16 strings that outlive the call. Without
    // MOVEFILE_REPLACE_EXISTING the move fails if the destination exists.
    let ok = unsafe { MoveFileExW(from_w.as_ptr(), to_w.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Leaves a race with another writer of the same name between the check and the rename; that
/// writer runs as the same user, which the threat model accepts (plan 4.5).
#[cfg(unix)]
fn rename_if_absent(from: &Path, to: &Path) -> io::Result<()> {
    if fs::symlink_metadata(to).is_ok() {
        return Err(io::ErrorKind::AlreadyExists.into());
    }
    fs::rename(from, to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;
    use cloakframe_core::item::{Item, ItemId, ReviewMode};
    use std::io::Write;

    fn permit() -> PublishPermit {
        let mut item = Item::new(ItemId(1));
        item.finish_analysis().unwrap();
        item.route(ReviewMode::Off, &[]).unwrap();
        item.start_render().unwrap();
        item.mark_verified().unwrap();
        PublishPermit::issue(&item, &[]).unwrap()
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn writes_under_a_hidden_name_until_published() {
        let dir = TestDir::new();
        let destination = dir.path().join("nested/result.png");
        let mut out = TempOutput::create(&destination).unwrap();
        out.file().write_all(b"pixels").unwrap();
        let during = names(&dir.path().join("nested"));
        assert_eq!(during.len(), 1);
        assert!(during[0].starts_with(TEMP_PREFIX) && during[0].ends_with(".png"));
        assert!(!destination.exists());

        assert_eq!(out.publish(&permit()).unwrap(), destination);
        assert_eq!(names(&dir.path().join("nested")), ["result.png"]);
        assert_eq!(fs::read(&destination).unwrap(), b"pixels");
    }

    #[test]
    fn never_replaces_an_existing_file() {
        let dir = TestDir::new();
        let destination = dir.write("result.png");
        let mut out = TempOutput::create(&destination).unwrap();
        out.file().write_all(b"new").unwrap();
        assert_eq!(
            out.publish(&permit()).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&destination).unwrap(), b"x");
        assert_eq!(names(dir.path()), ["result.png"]);
    }

    #[test]
    fn of_two_concurrent_writers_exactly_one_publishes() {
        let dir = TestDir::new();
        let destination = dir.path().join("result.png");
        let barrier = std::sync::Barrier::new(2);
        let results: Vec<io::Result<PathBuf>> = std::thread::scope(|s| {
            let writers: Vec<_> = [b"first".as_slice(), b"second".as_slice()]
                .into_iter()
                .map(|bytes| {
                    let (destination, barrier) = (&destination, &barrier);
                    s.spawn(move || {
                        let mut out = TempOutput::create(destination).unwrap();
                        out.file().write_all(bytes).unwrap();
                        barrier.wait();
                        out.publish(&permit())
                    })
                })
                .collect();
            writers.into_iter().map(|w| w.join().unwrap()).collect()
        });
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        let content = fs::read(&destination).unwrap();
        assert!(content == b"first" || content == b"second");
        assert_eq!(names(dir.path()), ["result.png"]);
    }

    #[test]
    fn dropping_an_unpublished_output_removes_it() {
        let dir = TestDir::new();
        let mut out = TempOutput::create(&dir.path().join("result.mp4")).unwrap();
        out.file().write_all(b"partial").unwrap();
        fs::write(out.temp_path(), b"written by path").unwrap();
        drop(out);
        assert!(names(dir.path()).is_empty());
    }

    #[test]
    fn publishes_data_written_through_the_temp_path() {
        let dir = TestDir::new();
        let destination = dir.path().join("clip.mp4");
        let out = TempOutput::create(&destination).unwrap();
        fs::write(out.temp_path(), b"encoded").unwrap();
        out.publish(&permit()).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"encoded");
    }

    #[test]
    fn removes_only_stale_temp_files() {
        let dir = TestDir::new();
        let stale = dir.write(".cloakframe-tmp-old.jpg");
        let fresh = dir.write(".cloakframe-tmp-new.jpg");
        let user = dir.write("photo.jpg");
        let old = SystemTime::now() - STALE_AFTER - Duration::from_secs(60);
        File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(old)
            .unwrap();
        File::options()
            .write(true)
            .open(&user)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(
            remove_stale_temp_files(dir.path(), SystemTime::now()).unwrap(),
            1
        );
        assert!(!stale.exists() && fresh.exists() && user.exists());
    }

    #[cfg(unix)]
    #[test]
    fn falls_back_when_exclusive_rename_is_unavailable() {
        let dir = TestDir::new();
        let from = dir.write(".cloakframe-tmp-a");
        let to = dir.path().join("a.jpg");
        rename_if_absent(&from, &to).unwrap();
        assert!(to.exists() && !from.exists());
        let again = dir.write(".cloakframe-tmp-b");
        assert_eq!(
            rename_if_absent(&again, &to).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
    }
}
