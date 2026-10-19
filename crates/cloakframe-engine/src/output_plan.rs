use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fs_case::NameRule;
use crate::scan::{MediaKind, ScannedFile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Conflict {
    /// A file or folder already has this name. It is never replaced.
    Existing,
    /// An earlier input in the run maps to the same name.
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedOutput {
    /// Relative to the output folder.
    pub relative: PathBuf,
    pub conflict: Option<Conflict>,
    /// A free name in the same folder to offer in review, when there is a conflict.
    pub suggestion: Option<PathBuf>,
}

/// Output paths mirror the input's relative path; videos are always written as MP4.
pub fn output_relative(file: &ScannedFile) -> PathBuf {
    match file.kind {
        MediaKind::Image => file.relative.clone(),
        MediaKind::Video => file.relative.with_extension("mp4"),
    }
}

/// Plans one output per input in order. Name collisions inside the run are judged by each
/// destination folder's own case rule (D10), probed once per folder.
pub fn plan_outputs(files: &[ScannedFile], output: &Path) -> Vec<PlannedOutput> {
    let mut cases: HashMap<PathBuf, NameRule> = HashMap::new();
    let mut taken: HashSet<(PathBuf, String)> = HashSet::new();
    let mut planned = Vec::with_capacity(files.len());
    for file in files {
        let relative = output_relative(file);
        let folder = relative.parent().unwrap_or(Path::new("")).to_path_buf();
        let case = *cases
            .entry(folder.clone())
            .or_insert_with(|| NameRule::probe(&output.join(&folder)));
        let name = relative
            .file_name()
            .map(OsStr::to_string_lossy)
            .unwrap_or_default()
            .into_owned();
        let conflict = if output.join(&relative).symlink_metadata().is_ok() {
            Some(Conflict::Existing)
        } else if taken.contains(&(folder.clone(), case.key(&name))) {
            Some(Conflict::Duplicate)
        } else {
            None
        };
        let suggestion = conflict.map(|_| {
            let free = free_name(&name, |candidate| {
                !taken.contains(&(folder.clone(), case.key(candidate)))
                    && output
                        .join(&folder)
                        .join(candidate)
                        .symlink_metadata()
                        .is_err()
            });
            folder.join(free)
        });
        let claimed = suggestion
            .as_ref()
            .and_then(|s| s.file_name())
            .map_or(name.clone(), |n| n.to_string_lossy().into_owned());
        taken.insert((folder.clone(), case.key(&claimed)));
        planned.push(PlannedOutput {
            relative,
            conflict,
            suggestion,
        });
    }
    planned
}

/// `photo.jpg` → `photo (2).jpg`, `photo (3).jpg`, … until `is_free` accepts one.
pub fn free_name(name: &str, is_free: impl Fn(&str) -> bool) -> String {
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    };
    (2..)
        .map(|n| format!("{stem} ({n}){extension}"))
        .find(|candidate| is_free(candidate))
        .expect("an unbounded sequence has a free name")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::identity;
    use crate::testdir::TestDir;

    fn scanned(dir: &TestDir, source: &str, relative: &str, kind: MediaKind) -> ScannedFile {
        let path = dir.write(source);
        ScannedFile {
            identity: identity(&path).unwrap(),
            path,
            relative: relative.into(),
            kind,
        }
    }

    #[test]
    fn mirrors_inputs_and_writes_videos_as_mp4() {
        let dir = TestDir::new();
        let out = dir.mkdir("out");
        let files = [
            scanned(&dir, "in/one.jpg", "one.jpg", MediaKind::Image),
            scanned(
                &dir,
                "in/nested/two.mov",
                "nested/two.mov",
                MediaKind::Video,
            ),
        ];
        let plan = plan_outputs(&files, &out);
        assert_eq!(plan[0].relative, Path::new("one.jpg"));
        assert_eq!(plan[1].relative, Path::new("nested/two.mp4"));
        assert!(
            plan.iter()
                .all(|p| p.conflict.is_none() && p.suggestion.is_none())
        );
    }

    #[test]
    fn an_existing_destination_gets_a_free_name_suggestion() {
        let dir = TestDir::new();
        let out = dir.mkdir("out");
        dir.write("out/one.jpg");
        dir.write("out/one (2).jpg");
        let files = [scanned(&dir, "in/one.jpg", "one.jpg", MediaKind::Image)];
        let plan = plan_outputs(&files, &out);
        assert_eq!(plan[0].conflict, Some(Conflict::Existing));
        assert_eq!(
            plan[0].suggestion.as_deref(),
            Some(Path::new("one (3).jpg"))
        );
    }

    #[test]
    fn duplicates_inside_the_run_follow_the_folder_case_rule() {
        let dir = TestDir::new();
        let out = dir.mkdir("out");
        let folds = NameRule::probe(&out).ignores_case;
        let files = [
            scanned(&dir, "a/Same.jpg", "Same.jpg", MediaKind::Image),
            scanned(&dir, "b/same.jpg", "same.jpg", MediaKind::Image),
            scanned(&dir, "c/same.jpg", "same.jpg", MediaKind::Image),
        ];
        let plan = plan_outputs(&files, &out);
        assert_eq!(plan[0].conflict, None);
        if folds {
            assert_eq!(plan[1].conflict, Some(Conflict::Duplicate));
            assert_eq!(
                plan[1].suggestion.as_deref(),
                Some(Path::new("same (2).jpg"))
            );
            assert_eq!(
                plan[2].suggestion.as_deref(),
                Some(Path::new("same (3).jpg"))
            );
        } else {
            assert_eq!(plan[1].conflict, None);
            assert_eq!(plan[2].conflict, Some(Conflict::Duplicate));
            assert_eq!(
                plan[2].suggestion.as_deref(),
                Some(Path::new("same (2).jpg"))
            );
        }
    }

    /// APFS and HFS+ compare names regardless of Unicode normalization: a name typed on Windows
    /// (NFC) and the same name made on a Mac (NFD) are one name there (review item 9).
    #[test]
    fn duplicates_inside_the_run_follow_the_folder_normalization_rule() {
        let dir = TestDir::new();
        let out = dir.mkdir("out");
        let (nfc, nfd) = (
            "\u{c0ac}\u{c9c4}.jpg",
            "\u{1109}\u{1161}\u{110c}\u{1175}\u{11ab}.jpg",
        );
        std::fs::write(out.join(nfc), b"x").unwrap();
        let folds = out.join(nfd).exists();
        std::fs::remove_file(out.join(nfc)).unwrap();
        let files = [
            scanned(&dir, &format!("a/{nfc}"), nfc, MediaKind::Image),
            scanned(&dir, &format!("b/{nfd}"), nfd, MediaKind::Image),
        ];
        let plan = plan_outputs(&files, &out);
        assert_eq!(plan[0].conflict, None);
        let want = folds.then_some(Conflict::Duplicate);
        assert_eq!(plan[1].conflict, want, "folds: {folds}");
    }

    #[test]
    fn free_names_keep_the_extension() {
        assert_eq!(free_name("photo.jpg", |_| true), "photo (2).jpg");
        assert_eq!(free_name("archive.tar.gz", |_| true), "archive.tar (2).gz");
        assert_eq!(free_name(".hidden", |_| true), ".hidden (2)");
        assert_eq!(free_name("README", |c| c != "README (2)"), "README (3)");
    }
}
