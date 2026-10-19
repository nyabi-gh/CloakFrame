use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::hash::{BuildHasher, RandomState};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use cloakframe_core::item::PublishPermit;

use crate::run::Note;

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
        let (file, temp) = create_temp(dir, destination.extension())?;
        Ok(Self {
            file: Some(file),
            temp,
            destination: destination.to_path_buf(),
            published: false,
        })
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
    /// `io::ErrorKind::AlreadyExists` if anything has appeared under that name. Once the output
    /// is visible under its final name it is published; what fails after that is returned as a
    /// warning, not an error.
    pub fn publish(self, permit: &PublishPermit) -> io::Result<Published> {
        self.publish_with(permit, sync_folder)
    }

    fn publish_with(
        mut self,
        _permit: &PublishPermit,
        sync_folder: impl FnOnce(&Path) -> io::Result<()>,
    ) -> io::Result<Published> {
        self.file.take();
        // Reopened so that data written through `temp_path` by another process is flushed too.
        File::options().write(true).open(&self.temp)?.sync_all()?;
        let mut warnings = Vec::new();
        if let Some(left) = rename_no_replace(&self.temp, &self.destination)? {
            warnings.push(AfterPublish::TempNameLeft(left));
        }
        self.published = true;
        if let Some(dir) = self.destination.parent()
            && let Err(e) = sync_folder(dir)
        {
            warnings.push(AfterPublish::FolderNotSynced(e));
        }
        Ok(Published {
            path: self.destination.clone(),
            warnings,
        })
    }
}

#[derive(Debug)]
pub struct Published {
    pub path: PathBuf,
    pub warnings: Vec<AfterPublish>,
}

/// A step after the output became visible that did not complete.
#[derive(Debug)]
pub enum AfterPublish {
    /// The folder could not be flushed, so a crash soon after could still undo the rename.
    FolderNotSynced(io::Error),
    /// The exclusive-rename fallback (hard link) could not remove the temp name.
    TempNameLeft(io::Error),
}

impl AfterPublish {
    pub fn note(&self) -> Note {
        match self {
            Self::FolderNotSynced(_) => Note::FolderNotFlushed,
            Self::TempNameLeft(_) => Note::TempFileLeft,
        }
    }
}

/// A new empty file in `dir` under a hidden temporary name ending in `extension`, with default
/// permissions (D4).
pub(crate) fn create_temp(dir: &Path, extension: Option<&OsStr>) -> io::Result<(File, PathBuf)> {
    loop {
        let mut candidate = OsString::from(TEMP_PREFIX);
        candidate.push(format!(
            "{:016x}",
            RandomState::new().hash_one(SystemTime::now())
        ));
        if let Some(extension) = extension {
            candidate.push(".");
            candidate.push(extension);
        }
        let temp = dir.join(&candidate);
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => return Ok((file, temp)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Makes a rename in `dir` survive a crash. Windows has no such call for folders; the rename
/// itself is written through (`MOVEFILE_WRITE_THROUGH`).
pub(crate) fn sync_folder(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    #[cfg(windows)]
    let _ = dir;
    Ok(())
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

/// Returns the error of removing the temp name when the hard-link fallback published the file but
/// could not remove that name.
#[cfg(unix)]
fn rename_no_replace(from: &Path, to: &Path) -> io::Result<Option<io::Error>> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    use rustix::io::Errno;

    match renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE) {
        Ok(()) => return Ok(None),
        // The filesystem or kernel lacks an exclusive rename (FAT, exFAT, some network volumes).
        // NOTSUP and OPNOTSUPP are the same value on Linux but not on macOS.
        Err(e) if [Errno::INVAL, Errno::NOSYS, Errno::NOTSUP, Errno::OPNOTSUPP].contains(&e) => {}
        Err(e) => return Err(e.into()),
    }
    match fs::hard_link(from, to) {
        Ok(()) => return Ok(fs::remove_file(from).err()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Err(e),
        Err(_) => {}
    }
    rename_if_absent(from, to).map(|()| None)
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn rename_no_replace(from: &Path, to: &Path) -> io::Result<Option<io::Error>> {
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};

    let (from_w, to_w) = (verbatim(from)?, verbatim(to)?);
    // SAFETY: both buffers are NUL-terminated UTF-16 strings that outlive the call. Without
    // MOVEFILE_REPLACE_EXISTING the move fails if the destination exists.
    let ok = unsafe { MoveFileExW(from_w.as_ptr(), to_w.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(None)
}

/// The NUL-terminated `\\?\` form of `path`. std's file functions switch to it for long paths;
/// a direct Win32 call has to as well, or paths past MAX_PATH fail (the CLI has no long path
/// manifest). Such paths skip Win32 normalization, so the path is made absolute and normalized
/// first.
#[cfg(windows)]
fn verbatim(path: &Path) -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;

    let absolute: Vec<u16> = std::path::absolute(path)?
        .as_os_str()
        .encode_wide()
        .collect();
    let starts = |prefix: &str| absolute.starts_with(&prefix.encode_utf16().collect::<Vec<_>>());
    let (prefix, rest) = if starts(r"\\?\") || starts(r"\\.\") {
        ("", &absolute[..])
    } else if starts(r"\\") {
        (r"\\?\UNC\", &absolute[2..])
    } else {
        (r"\\?\", &absolute[..])
    };
    Ok(prefix
        .encode_utf16()
        .chain(rest.iter().copied())
        .chain([0])
        .collect())
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
    use crate::test_support::{TestDir, permit};
    use std::io::Write;

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

        assert_eq!(out.publish(&permit()).unwrap().path, destination);
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
        let results: Vec<io::Result<Published>> = std::thread::scope(|s| {
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

    /// A volume that cannot flush folders (some network and FUSE volumes) still gets the output:
    /// it is visible under its final name, so the item is published and told so.
    #[test]
    fn a_folder_that_cannot_be_flushed_still_publishes() {
        let dir = TestDir::new();
        let destination = dir.path().join("out/photo.jpg");
        let mut out = TempOutput::create(&destination).unwrap();
        out.file().write_all(b"masked").unwrap();
        let published = out
            .publish_with(&permit(), |_| {
                Err(io::Error::other("folder sync not supported"))
            })
            .unwrap();
        assert_eq!(published.path, destination);
        assert!(matches!(
            published.warnings[..],
            [AfterPublish::FolderNotSynced(_)]
        ));
        assert_eq!(names(&dir.path().join("out")), ["photo.jpg"]);
    }

    /// Windows: std reaches paths past MAX_PATH (260) by turning them into `\\?\` paths, and
    /// the rename has to as well, or only the temp file is written (review item 16).
    #[test]
    fn a_destination_past_max_path_publishes() {
        let dir = TestDir::new();
        let deep = (0..6).fold(dir.path().to_path_buf(), |p, i| {
            p.join(format!("{i}-a-folder-name-long-enough-to-add-up-quickly"))
        });
        let destination = deep.join("photo.jpg");
        assert!(destination.as_os_str().len() > 300);
        let mut out = TempOutput::create(&destination).unwrap();
        out.file().write_all(b"masked").unwrap();
        assert_eq!(out.publish(&permit()).unwrap().path, destination);
        assert_eq!(names(&deep), ["photo.jpg"]);
        assert_eq!(fs::read(&destination).unwrap(), b"masked");
    }
}
