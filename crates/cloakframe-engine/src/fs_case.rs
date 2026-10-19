use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use unicode_normalization::UnicodeNormalization;

use crate::identity::identity;
use crate::publish::TEMP_PREFIX;

/// Which names a folder treats as the same name (D10). Every check that compares names which do
/// not exist yet, such as planned output names, asks this. Names of existing files are compared
/// by identity instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameRule {
    /// Names that differ only in letter case.
    pub ignores_case: bool,
    /// Names that differ only in Unicode normalization (NFC and NFD), as on APFS and HFS+. A
    /// name typed on Windows is usually NFC and one made in the macOS Finder NFD.
    pub ignores_normalization: bool,
}

impl NameRule {
    /// Probes the nearest existing folder at or above `dir`, after following links, by looking
    /// up names inside it in another letter case and another normalization: entries already
    /// there, or else a temporary file. Volumes can differ from the OS default (APFS
    /// case-sensitive volumes, case-insensitive ext4 folders, NTFS per-folder case sensitivity),
    /// and a folder's own name lives in its parent, which can be on another volume (a mount
    /// point, a link).
    pub fn probe(dir: &Path) -> Self {
        let absolute = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
        for folder in absolute.ancestors() {
            let Ok(folder) = std::fs::canonicalize(folder) else {
                continue;
            };
            let case = probe_entry(&folder, flip_first_letter);
            let normalization = probe_entry(&folder, other_normalization);
            let (case, normalization) = match (case, normalization) {
                (Some(c), Some(n)) => (c, n),
                _ => match probe_with_file(&folder) {
                    Some((c, n)) => (case.unwrap_or(c), normalization.unwrap_or(n)),
                    None => continue,
                },
            };
            return Self {
                ignores_case: case,
                ignores_normalization: normalization,
            };
        }
        Self {
            ignores_case: cfg!(any(windows, target_os = "macos")),
            ignores_normalization: cfg!(target_os = "macos"),
        }
    }

    /// The key under which two names collide in a folder with this rule.
    pub fn key(self, name: &str) -> String {
        let normalized = |s: &str| -> String {
            if self.ignores_normalization {
                s.nfc().collect()
            } else {
                s.to_owned()
            }
        };
        if self.ignores_case {
            normalized(&normalized(name).to_lowercase())
        } else {
            normalized(name)
        }
    }
}

/// Whether `name` and its `variant` are one file in `folder`; None when there is no variant or
/// `name` cannot be read.
fn same_file(folder: &Path, name: &str, variant: fn(&str) -> Option<String>) -> Option<bool> {
    let other = variant(name)?;
    let original = identity(&folder.join(name)).ok()?;
    Some(identity(&folder.join(other)).is_ok_and(|o| o == original))
}

/// Links are skipped: two differently spelled links to one target would look like one name.
fn probe_entry(folder: &Path, variant: fn(&str) -> Option<String>) -> Option<bool> {
    std::fs::read_dir(folder)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| !t.is_symlink()))
        .find_map(|e| same_file(folder, e.file_name().to_str()?, variant))
}

/// Returns (ignores case, ignores normalization) from a temporary file whose name has both a
/// letter and a precomposed character.
fn probe_with_file(folder: &Path) -> Option<(bool, bool)> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let name = format!(
        "{TEMP_PREFIX}case-\u{e9}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let path = folder.join(&name);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .ok()?;
    // HFS+ stores names as NFD, so the name is looked up as the folder lists it.
    let stored = std::fs::read_dir(folder)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .find(|n| n.nfc().eq(name.chars()));
    let rule = stored.and_then(|stored| {
        Some((
            same_file(folder, &stored, flip_first_letter)?,
            same_file(folder, &stored, other_normalization)?,
        ))
    });
    // A file that cannot be removed now carries the temp prefix: it is recognizable, and in an
    // output folder the next run's cleanup of stale temp files removes it.
    let _ = std::fs::remove_file(&path);
    rule
}

fn other_normalization(name: &str) -> Option<String> {
    let nfc: String = name.nfc().collect();
    if nfc != name {
        return Some(nfc);
    }
    let nfd: String = name.nfd().collect();
    (nfd != name).then_some(nfd)
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
        let folds = dir.join("PROBE.TXT").exists();
        std::fs::remove_file(dir.join("probe.txt")).unwrap();
        folds
    }

    fn normalizes_here(dir: &Path) -> bool {
        std::fs::write(dir.join("caf\u{e9}"), b"x").unwrap();
        let normalizes = dir.join("cafe\u{301}").exists();
        std::fs::remove_file(dir.join("caf\u{e9}")).unwrap();
        normalizes
    }

    #[test]
    fn probe_matches_what_the_filesystem_does() {
        let dir = TestDir::new();
        let out = dir.mkdir("out");
        let expected = NameRule {
            ignores_case: folds_here(&out),
            ignores_normalization: normalizes_here(&out),
        };
        assert_eq!(NameRule::probe(&out), expected);
        assert_eq!(NameRule::probe(&out.join("not/created")), expected);
    }

    #[test]
    fn probing_an_empty_folder_leaves_it_empty() {
        let dir = TestDir::new();
        let out = dir.mkdir("out");
        let expected = NameRule::probe(&dir.path().join("probe-elsewhere"));
        assert_eq!(NameRule::probe(&out), expected);
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
        assert!(!NameRule::probe(&volume.root).ignores_case);
        assert!(!NameRule::probe(&volume.root.join("not/created")).ignores_case);
        let elsewhere = dir.mkdir("elsewhere");
        let link = volume.root.join("link");
        std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
        assert!(NameRule::probe(&link).ignores_case);
        assert!(NameRule::probe(&link.join("out")).ignores_case);
    }

    #[test]
    fn keys_collide_only_when_the_folder_ignores_the_difference() {
        let rule = |ignores_case, ignores_normalization| NameRule {
            ignores_case,
            ignores_normalization,
        };
        let (nfc, nfd) = ("Caf\u{e9}.jpg", "cafe\u{301}.jpg");
        assert_eq!(rule(true, true).key(nfc), rule(true, true).key(nfd));
        assert_ne!(rule(true, false).key(nfc), rule(true, false).key(nfd));
        assert_ne!(rule(false, true).key(nfc), rule(false, true).key(nfd));
        assert_eq!(
            rule(false, true).key("caf\u{e9}"),
            rule(false, true).key("cafe\u{301}")
        );
        assert_eq!(
            rule(true, false).key("Same.JPG"),
            rule(true, false).key("same.jpg")
        );
        assert_ne!(
            rule(false, false).key("Same.JPG"),
            rule(false, false).key("same.jpg")
        );
    }

    #[test]
    fn flips_the_first_cased_letter() {
        assert_eq!(flip_first_letter("123abc").as_deref(), Some("123Abc"));
        assert_eq!(flip_first_letter("사진A").as_deref(), Some("사진a"));
        assert_eq!(flip_first_letter("123_사진"), None);
    }
}
