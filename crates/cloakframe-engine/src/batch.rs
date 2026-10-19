use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use cloakframe_core::item::{ItemId, ItemState, ReviewMode};
use cloakframe_core::review::{Digest, PhotoEdits, ReviewKey, ReviewRevision};
use sha2::{Digest as _, Sha256};

use crate::identity::SourceStamp;
use crate::output_plan::{PlannedOutput, plan_outputs};
use crate::run::{Fingerprint, Job, JobItem, RunReport, Start};
use crate::scan::ScannedFile;
use crate::session::{FORMAT_VERSION, Session, SessionItem};

pub fn sha256_file(path: &Path) -> io::Result<Digest> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(Digest(hasher.finalize().into()))
}

/// One job item per scanned file, in scan order. An output whose name is taken goes to review
/// even in automatic mode.
pub fn job_from_scan(
    files: &[ScannedFile],
    output: &Path,
    review: ReviewMode,
    workers: usize,
) -> (Job, Vec<PlannedOutput>) {
    let plan = plan_outputs(files, output);
    let items = files
        .iter()
        .zip(&plan)
        .enumerate()
        .map(|(i, (file, planned))| JobItem {
            id: ItemId(i as u64),
            input: file.path.clone(),
            destination: output.join(&planned.relative),
            start: Start::Fresh {
                needs_review: planned.conflict.is_some(),
            },
        })
        .collect();
    (
        Job {
            items,
            review,
            workers,
        },
        plan,
    )
}

/// Why an item the run held for review is not in its review session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeftOut {
    /// The run kept no record of the source it analyzed.
    NotAnalyzed,
    /// The source changed or vanished since it was read for analysis.
    SourceChanged,
    /// The source could not be read again.
    Unreadable(String),
}

impl fmt::Display for LeftOut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnalyzed => f.write_str("the item was not analyzed; run it again"),
            Self::SourceChanged => {
                f.write_str("the source changed or is gone since analysis; run it again")
            }
            Self::Unreadable(e) => write!(f, "cannot read the source again ({e}); run it again"),
        }
    }
}

/// The review session for the items a run left in `NeedsReview` (`None` if there are none), and
/// the held items that cannot be reviewed, which need a new run.
pub fn session_from_report(
    report: &RunReport,
    plan: &[PlannedOutput],
    output: &Path,
    fingerprint: Fingerprint,
) -> (Option<Session>, Vec<(ItemId, LeftOut)>) {
    let mut items = Vec::new();
    let mut left_out = Vec::new();
    for (outcome, planned) in report.items.iter().zip(plan) {
        if outcome.state != ItemState::NeedsReview {
            continue;
        }
        let Some(source) = outcome.analysis.source else {
            left_out.push((outcome.item, LeftOut::NotAnalyzed));
            continue;
        };
        if SourceStamp::of(&outcome.input).ok() != Some(source) {
            left_out.push((outcome.item, LeftOut::SourceChanged));
            continue;
        }
        let input = match sha256_file(&outcome.input) {
            Ok(digest) => digest,
            Err(e) => {
                left_out.push((outcome.item, LeftOut::Unreadable(e.to_string())));
                continue;
            }
        };
        let key = ReviewKey {
            input,
            models: fingerprint.models,
            settings: fingerprint.settings,
            // Photos have no timeline; videos bind their TimelineSpec here from M4.
            timeline: Digest(Sha256::digest(b"").into()),
        };
        items.push(SessionItem {
            item: outcome.item,
            input: outcome.input.clone(),
            destination: planned.relative.clone(),
            conflict: planned.conflict,
            suggestion: planned.suggestion.clone(),
            source,
            revision: ReviewRevision::first(key),
            detections: outcome.analysis.detections.clone(),
            evidence: outcome.analysis.evidence.clone(),
            edits: PhotoEdits::default(),
        });
    }
    let session = (!items.is_empty()).then(|| Session {
        version: FORMAT_VERSION,
        output: output.to_path_buf(),
        items,
    });
    (session, left_out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;

    /// An item held for review whose source changed or vanished cannot be reviewed; it is
    /// reported with the reason instead of silently missing from the session.
    #[test]
    fn items_left_out_of_the_session_are_reported() {
        use crate::run::{Analysis, ItemOutcome};
        let dir = TestDir::new();
        let outcome = |i: u64, name: &str| {
            let input = dir.write(&format!("in/{name}"));
            ItemOutcome {
                item: ItemId(i),
                input: input.clone(),
                destination: dir.path().join("out").join(name),
                state: ItemState::NeedsReview,
                analysis: Analysis {
                    source: Some(SourceStamp::of(&input).unwrap()),
                    ..Analysis::default()
                },
                failure: None,
                notes: Vec::new(),
            }
        };
        let report = RunReport {
            items: vec![
                outcome(0, "kept.jpg"),
                outcome(1, "changed.jpg"),
                outcome(2, "gone.jpg"),
            ],
        };
        std::fs::write(&report.items[1].input, b"edited after analysis").unwrap();
        std::fs::remove_file(&report.items[2].input).unwrap();
        let plan: Vec<PlannedOutput> = report
            .items
            .iter()
            .map(|o| PlannedOutput {
                relative: o.destination.file_name().unwrap().into(),
                conflict: None,
                suggestion: None,
            })
            .collect();
        let fingerprint = Fingerprint {
            models: Digest([0; 32]),
            settings: Digest([0; 32]),
        };
        let (session, left_out) =
            session_from_report(&report, &plan, &dir.path().join("out"), fingerprint);
        let session = session.unwrap();
        assert_eq!(session.items.len(), 1);
        assert_eq!(session.items[0].item, ItemId(0));
        assert_eq!(
            left_out,
            [
                (ItemId(1), LeftOut::SourceChanged),
                (ItemId(2), LeftOut::SourceChanged)
            ]
        );
    }

    #[test]
    fn hashes_files_with_sha256() {
        let dir = TestDir::new();
        let path = dir.path().join("abc.txt");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap().to_string(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
