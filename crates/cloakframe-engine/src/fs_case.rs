use std::ffi::OsStr;
use std::path::Path;

use crate::identity::identity;

/// Whether a folder treats names that differ only in letter case as the same name (D10). Every
/// check that compares names which do not exist yet, such as planned output names, asks this.
/// Names of existing files are compared by identity instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameCase {
    Sensitive,
    Insensitive,
}

impl NameCase {
    /// Probes the nearest existing folder at or above `dir` by looking up one of its own name
    /// components with one letter's case flipped. Volumes can differ from the OS default (APFS
    /// case-sensitive volumes, case-insensitive ext4 folders, NTFS per-folder case sensitivity).
    pub fn probe(dir: &Path) -> Self {
        let absolute = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
        for probe in absolute.ancestors() {
            let Some(name) = probe.file_name().and_then(OsStr::to_str) else {
                continue;
            };
            let Some(flipped) = flip_first_letter(name) else {
                continue;
            };
            let Ok(original) = identity(probe) else {
                continue;
            };
            return match identity(&probe.with_file_name(flipped)) {
                Ok(other) if other == original => Self::Insensitive,
                _ => Self::Sensitive,
            };
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
