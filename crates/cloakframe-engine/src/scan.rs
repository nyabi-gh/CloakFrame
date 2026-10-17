use std::collections::{BTreeMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::cancel::CancelToken;
use crate::identity::{FileIdentity, identity};

const IMAGE_EXTENSIONS: [&str; 7] = ["jpg", "jpeg", "png", "bmp", "tif", "tiff", "webp"];
const VIDEO_EXTENSIONS: [&str; 4] = ["mp4", "mov", "m4v", "webm"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
}

#[derive(Debug, Clone)]
pub struct ScanRequest {
    pub inputs: Vec<PathBuf>,
    pub recursive: bool,
    pub include_videos: bool,
    /// Skipped while walking so a previous run's results are not scanned as inputs.
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFile {
    pub path: PathBuf,
    /// Relative to the input folder it was found in, or the file name for a file input. Output
    /// paths mirror it.
    pub relative: PathBuf,
    pub kind: MediaKind,
    pub identity: FileIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanIssue {
    pub path: PathBuf,
    pub kind: io::ErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScanResult {
    pub files: Vec<ScannedFile>,
    /// Unsupported files by lowercase extension without the dot ("" for none).
    pub skipped: BTreeMap<String, usize>,
    pub issues: Vec<ScanIssue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

pub fn scan(request: &ScanRequest, cancel: &CancelToken) -> Result<ScanResult, Cancelled> {
    let output = request.output.as_deref().and_then(|p| identity(p).ok());
    let mut scanner = Scanner {
        request,
        cancel,
        output,
        seen: HashSet::new(),
        result: ScanResult::default(),
    };
    for input in &request.inputs {
        scanner.input(input)?;
    }
    let mut result = scanner.result;
    result.files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

struct Scanner<'a> {
    request: &'a ScanRequest,
    cancel: &'a CancelToken,
    output: Option<FileIdentity>,
    seen: HashSet<FileIdentity>,
    result: ScanResult,
}

impl Scanner<'_> {
    fn issue(&mut self, path: &Path, error: &io::Error) {
        self.result.issues.push(ScanIssue {
            path: path.to_path_buf(),
            kind: error.kind(),
        });
    }

    fn input(&mut self, input: &Path) -> Result<(), Cancelled> {
        match fs::metadata(input) {
            Err(e) => self.issue(input, &e),
            Ok(m) if m.is_dir() => self.walk(input)?,
            Ok(_) => {
                let name = input.file_name().map(PathBuf::from).unwrap_or_default();
                self.file(input, name);
            }
        }
        Ok(())
    }

    fn walk(&mut self, root: &Path) -> Result<(), Cancelled> {
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            if self.output.is_some() && identity(&dir).ok() == self.output {
                continue;
            }
            let entries = match fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(e) => {
                    self.issue(&dir, &e);
                    continue;
                }
            };
            for entry in entries {
                if self.cancel.is_cancelled() {
                    return Err(Cancelled);
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(e) => {
                        self.issue(&dir, &e);
                        break;
                    }
                };
                let path = entry.path();
                if is_hidden_or_system(&entry) {
                    continue;
                }
                // Symbolic links and junctions to folders are not followed; links to files are.
                match entry.file_type() {
                    Ok(t) if t.is_dir() => {
                        if self.request.recursive {
                            pending.push(path);
                        }
                    }
                    Ok(_) => match fs::metadata(&path) {
                        Ok(m) if m.is_file() => {
                            let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                            self.file(&path, relative);
                        }
                        Ok(_) => {}
                        Err(e) => self.issue(&path, &e),
                    },
                    Err(e) => self.issue(&path, &e),
                }
            }
        }
        Ok(())
    }

    fn file(&mut self, path: &Path, relative: PathBuf) {
        let extension = path
            .extension()
            .and_then(OsStr::to_str)
            .unwrap_or("")
            .to_lowercase();
        let kind = if IMAGE_EXTENSIONS.contains(&extension.as_str()) {
            MediaKind::Image
        } else if self.request.include_videos && VIDEO_EXTENSIONS.contains(&extension.as_str()) {
            MediaKind::Video
        } else {
            *self.result.skipped.entry(extension).or_default() += 1;
            return;
        };
        let identity = match identity(path) {
            Ok(identity) => identity,
            Err(e) => return self.issue(path, &e),
        };
        if self.seen.insert(identity) {
            self.result.files.push(ScannedFile {
                path: path.to_path_buf(),
                relative,
                kind,
                identity,
            });
        }
    }
}

fn is_hidden_or_system(entry: &fs::DirEntry) -> bool {
    let name = entry.file_name();
    let name = name.to_string_lossy();
    if name.starts_with('.')
        || name.eq_ignore_ascii_case("Thumbs.db")
        || name.eq_ignore_ascii_case("desktop.ini")
    {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN: u32 = 0x2;
        const SYSTEM: u32 = 0x4;
        if let Ok(m) = entry.metadata() {
            return m.file_attributes() & (HIDDEN | SYSTEM) != 0;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;

    fn request(inputs: &[PathBuf], recursive: bool) -> ScanRequest {
        ScanRequest {
            inputs: inputs.to_vec(),
            recursive,
            include_videos: true,
            output: None,
        }
    }

    fn run(request: &ScanRequest) -> ScanResult {
        scan(request, &CancelToken::new()).unwrap()
    }

    fn relatives(result: &ScanResult) -> Vec<String> {
        let mut r: Vec<_> = result
            .files
            .iter()
            .map(|f| f.relative.to_string_lossy().replace('\\', "/"))
            .collect();
        r.sort();
        r
    }

    #[test]
    fn recurses_on_request_and_deduplicates_by_identity() {
        let dir = TestDir::new();
        let first = dir.write("a/one.JPG");
        dir.write("a/nested/two.png");
        dir.write("b/notes.txt");
        let a = dir.path().join("a");

        assert_eq!(
            relatives(&run(&request(std::slice::from_ref(&a), false))),
            ["one.JPG"]
        );
        let recursive = run(&request(&[a, first], true));
        assert_eq!(relatives(&recursive), ["nested/two.png", "one.JPG"]);
    }

    #[test]
    fn counts_unsupported_files_by_type_and_ignores_system_files() {
        let dir = TestDir::new();
        for name in [
            "photos/one.jpg",
            "photos/two.HEIC",
            "photos/nested/three.heic",
            "photos/clip.mkv",
            "photos/movie.mp4",
            "photos/README",
            "photos/.DS_Store",
            "photos/Thumbs.db",
            "photos/desktop.ini",
            "photos/.hidden/four.jpg",
        ] {
            dir.write(name);
        }
        let photos = [dir.path().join("photos")];
        let media = run(&request(&photos, true));
        assert_eq!(relatives(&media), ["movie.mp4", "one.jpg"]);
        assert_eq!(
            media
                .files
                .iter()
                .find(|f| f.relative == Path::new("movie.mp4"))
                .unwrap()
                .kind,
            MediaKind::Video
        );
        let expected: BTreeMap<String, usize> =
            [("heic".into(), 2), ("mkv".into(), 1), ("".into(), 1)].into();
        assert_eq!(media.skipped, expected);

        let images_only = run(&ScanRequest {
            include_videos: false,
            ..request(&photos, true)
        });
        assert_eq!(images_only.skipped["mp4"], 1);
        assert_eq!(images_only.skipped["mkv"], 1);
    }

    #[test]
    fn reports_missing_inputs_and_continues() {
        let dir = TestDir::new();
        dir.write("a/one.JPG");
        let a = dir.path().join("a");
        let result = run(&request(
            &[a.clone(), a.join("gone.jpg"), dir.path().join("nowhere")],
            true,
        ));
        assert_eq!(result.files.len(), 1);
        let reported: Vec<_> = result
            .issues
            .iter()
            .map(|i| (i.path.file_name().unwrap().to_owned(), i.kind))
            .collect();
        assert_eq!(
            reported,
            [
                ("gone.jpg".into(), io::ErrorKind::NotFound),
                ("nowhere".into(), io::ErrorKind::NotFound)
            ]
        );
        assert!(run(&request(&[a], true)).issues.is_empty());
    }

    // D10: duplicates are decided by identity, so the same file through a differently cased path
    // is one input exactly when the filesystem says so, and so is a hard link.
    #[test]
    fn the_same_file_under_another_name_is_one_input() {
        let dir = TestDir::new();
        let photo = dir.write("photo.jpg");
        let upper = dir.path().join("PHOTO.JPG");
        let linked = dir.path().join("linked.jpg");
        std::fs::hard_link(&photo, &linked).unwrap();
        let result = run(&request(&[photo, upper.clone(), linked], false));
        assert_eq!(result.files.len(), 1);
        let folds = upper.exists();
        assert_eq!(result.issues.len(), usize::from(!folds));
    }

    #[test]
    fn differently_cased_folders_are_two_inputs_only_where_names_are_case_sensitive() {
        let dir = TestDir::new();
        dir.write("Photos/a.jpg");
        let lower = dir.path().join("photos");
        let folds = lower.exists();
        if !folds {
            dir.write("photos/a.jpg");
        }
        let result = run(&request(&[dir.path().join("Photos"), lower], false));
        assert_eq!(result.files.len(), if folds { 1 } else { 2 });
    }

    #[test]
    fn skips_the_output_folder_inside_an_input() {
        let dir = TestDir::new();
        dir.write("Photos/a.jpg");
        dir.write("Photos/out/a.jpg");
        let output = if dir.path().join("photos").exists() {
            dir.path().join("photos/out")
        } else {
            dir.path().join("Photos/out")
        };
        let result = run(&ScanRequest {
            output: Some(output),
            ..request(&[dir.path().join("Photos")], true)
        });
        assert_eq!(relatives(&result), ["a.jpg"]);
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_folder_links_but_follows_file_links() {
        let dir = TestDir::new();
        dir.write("chosen/inside.jpg");
        let outside = dir.write("elsewhere/outside.jpg");
        let chosen = dir.path().join("chosen");
        std::os::unix::fs::symlink(dir.path().join("elsewhere"), chosen.join("folder-link"))
            .unwrap();
        std::os::unix::fs::symlink(dir.path(), chosen.join("loop")).unwrap();
        std::os::unix::fs::symlink(&outside, chosen.join("file-link.jpg")).unwrap();
        let result = run(&request(&[chosen], true));
        assert_eq!(relatives(&result), ["file-link.jpg", "inside.jpg"]);
    }

    #[cfg(windows)]
    #[test]
    fn does_not_follow_junctions() {
        let dir = TestDir::new();
        dir.write("chosen/inside.jpg");
        dir.write("elsewhere/outside.jpg");
        let status = std::process::Command::new("cmd.exe")
            .args(["/c", "mklink", "/J"])
            .arg(dir.path().join("chosen\\link"))
            .arg(dir.path().join("elsewhere"))
            .status()
            .unwrap();
        assert!(status.success());
        let result = run(&request(&[dir.path().join("chosen")], true));
        assert_eq!(relatives(&result), ["inside.jpg"]);
    }

    #[cfg(unix)]
    #[test]
    fn reports_unreadable_folders_and_continues() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TestDir::new();
        dir.write("input/top.png");
        dir.write("input/readable/visible.png");
        dir.write("input/denied/private.png");
        let denied = dir.path().join("input/denied");
        std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o000)).unwrap();
        let readable_anyway = std::fs::read_dir(&denied).is_ok();
        let result = run(&request(&[dir.path().join("input")], true));
        std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o755)).unwrap();
        if readable_anyway {
            return; // root bypasses folder permissions
        }
        assert_eq!(relatives(&result), ["readable/visible.png", "top.png"]);
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].kind, io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn stops_when_cancelled() {
        let dir = TestDir::new();
        dir.write("a/one.jpg");
        let cancel = CancelToken::new();
        cancel.cancel();
        assert_eq!(
            scan(&request(&[dir.path().join("a")], true), &cancel),
            Err(Cancelled)
        );
    }
}
