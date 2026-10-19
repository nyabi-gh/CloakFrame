use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::identity::identity;
use crate::publish::TEMP_PREFIX;

/// Whether a folder treats names that differ only in letter case as the same name (D10). Every
/// check that compares names which do not exist yet, such as planned output names, asks this.
/// Names of existing files are compared by identity instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameCase {
    Sensitive,
    Insensitive,
}

impl NameCase {
    /// Probes the nearest existing folder at or above `dir`, after following links, by looking
    /// up a name inside it with one letter's case flipped: an entry already there, or else a
    /// temporary file. Volumes can differ from the OS default (APFS case-sensitive volumes,
    /// case-insensitive ext4 folders, NTFS per-folder case sensitivity), and a folder's own name
    /// lives in its parent, which can be on another volume (a mount point, a link).
    pub fn probe(dir: &Path) -> Self {
        let absolute = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
        for folder in absolute.ancestors() {
            let Ok(folder) = std::fs::canonicalize(folder) else {
                continue;
            };
            if let Some(rule) = probe_entry(&folder).or_else(|| probe_with_file(&folder)) {
                return rule;
            }
        }
        if cfg!(any(windows, target_os = "macos")) {
            Self::Insensitive
        } else {
            Self::Sensitive
        }
    }

    /// The key under which two names collide in a folder with this rule.
    pub fn key(self, name: &str) -> String {
        match self {
            Self::Sensitive => name.to_owned(),
            Self::Insensitive => name.to_lowercase(),
        }
    }
}

fn compare(folder: &Path, name: &str) -> Option<NameCase> {
    let flipped = flip_first_letter(name)?;
    let original = identity(&folder.join(name)).ok()?;
    Some(match identity(&folder.join(flipped)) {
        Ok(other) if other == original => NameCase::Insensitive,
        _ => NameCase::Sensitive,
    })
}

/// Links are skipped: two differently cased links to one target would look like one name.
fn probe_entry(folder: &Path) -> Option<NameCase> {
    std::fs::read_dir(folder)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| !t.is_symlink()))
        .find_map(|e| compare(folder, e.file_name().to_str()?))
}

fn probe_with_file(folder: &Path) -> Option<NameCase> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let name = format!(
        "{TEMP_PREFIX}case-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let path = folder.join(&name);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .ok()?;
    let rule = compare(folder, &name);
    // A file that cannot be removed now carries the temp prefix: it is recognizable, and in an
    // output folder the next run's cleanup of stale temp files removes it.
    let _ = std::fs::remove_file(&path);
    rule
}

fn flip_first_letter(name: &str) -> Option<String> {
    let (at, ch) = name
        .char_indices()
        .find(|(_, c)| c.is_lowercase() != c.is_uppercase())?;
    let flipped: String = if ch.is_lowercase() {
        ch.to_uppercase().collect()
    } else {
        ch.to_lowercase().collect()
    };
    Some(format!(
        "{}{flipped}{}",
        &name[..at],
        &name[at + ch.len_utf8()..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;

    fn folds_here(dir: &Path) -> bool {
        std::fs::write(dir.join("probe.txt"), b"x").unwrap();
        dir.join("PROBE.TXT").exists()
    }

    #[test]
    fn probe_matches_what_the_filesystem_does() {
        let dir = TestDir::new();
        let out = dir.mkdir("out");
        let expected = if folds_here(&out) {
            NameCase::Insensitive
        } else {
            NameCase::Sensitive
        };
        assert_eq!(NameCase::probe(&out), expected);
        assert_eq!(NameCase::probe(&out.join("not/created")), expected);
    }

    #[test]
    fn probing_an_empty_folder_leaves_it_empty() {
        let dir = TestDir::new();
        let out = dir.mkdir("out");
        let expected = NameCase::probe(&dir.path().join("probe-elsewhere"));
        assert_eq!(NameCase::probe(&out), expected);
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0);
    }

    /// A case-sensitive APFS image mounted inside the (case-insensitive) temp folder, detached
    /// on drop.
    #[cfg(target_os = "macos")]
    struct CaseSensitiveVolume {
        root: std::path::PathBuf,
    }

    #[cfg(target_os = "macos")]
    impl CaseSensitiveVolume {
        fn mount(dir: &TestDir) -> Self {
            let image = dir.path().join("volume.dmg");
            let root = dir.mkdir("mnt");
            let hdiutil = |args: &[&std::ffi::OsStr]| {
                let status = std::process::Command::new("hdiutil")
                    .args(args)
                    .stdout(std::process::Stdio::null())
                    .status()
                    .unwrap();
                assert!(status.success(), "hdiutil {args:?}");
            };
            hdiutil(&[
                "create".as_ref(),
                "-size".as_ref(),
                "16m".as_ref(),
                "-fs".as_ref(),
                "Case-sensitive APFS".as_ref(),
                "-volname".as_ref(),
                "CloakFrameCase".as_ref(),
                image.as_os_str(),
            ]);
            hdiutil(&[
                "attach".as_ref(),
                "-nobrowse".as_ref(),
                "-mountpoint".as_ref(),
                root.as_os_str(),
                image.as_os_str(),
            ]);
            Self { root }
        }
    }

    #[cfg(target_os = "macos")]
    impl Drop for CaseSensitiveVolume {
        fn drop(&mut self) {
            let _ = std::process::Command::new("hdiutil")
                .args(["detach".as_ref(), "-force".as_ref(), self.root.as_os_str()])
                .stdout(std::process::Stdio::null())
                .status();
        }
    }

    /// D10 at volume boundaries: the rule is the folder's own, not that of the folder holding
    /// its name (review item 5).
    #[cfg(target_os = "macos")]
    #[test]
    fn the_rule_is_the_folders_own_not_its_parents() {
        let dir = TestDir::new();
        assert!(
            folds_here(dir.path()),
            "the temp folder is expected to ignore case"
        );
        let volume = CaseSensitiveVolume::mount(&dir);
        assert_eq!(NameCase::probe(&volume.root), NameCase::Sensitive);
        assert_eq!(
            NameCase::probe(&volume.root.join("not/created")),
            NameCase::Sensitive
        );
        let elsewhere = dir.mkdir("elsewhere");
        let link = volume.root.join("link");
        std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
        assert_eq!(NameCase::probe(&link), NameCase::Insensitive);
        assert_eq!(NameCase::probe(&link.join("out")), NameCase::Insensitive);
    }

    #[test]
    fn keys_collide_only_when_case_is_ignored() {
        assert_eq!(
            NameCase::Insensitive.key("Same.JPG"),
            NameCase::Insensitive.key("same.jpg")
        );
        assert_ne!(
            NameCase::Sensitive.key("Same.JPG"),
            NameCase::Sensitive.key("same.jpg")
        );
    }

    #[test]
    fn flips_the_first_cased_letter() {
        assert_eq!(flip_first_letter("123abc").as_deref(), Some("123Abc"));
        assert_eq!(flip_first_letter("사진A").as_deref(), Some("사진a"));
        assert_eq!(flip_first_letter("123_사진"), None);
    }
}
