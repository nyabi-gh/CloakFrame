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

/// The review session for the items a run left in `NeedsReview`, or `None` if there are none.
/// Items whose source changed or vanished since it was read for analysis are left out; they need
/// a new run.
pub fn session_from_report(
    report: &RunReport,
    plan: &[PlannedOutput],
    output: &Path,
    fingerprint: Fingerprint,
) -> io::Result<Option<Session>> {
    let mut items = Vec::new();
    for (outcome, planned) in report.items.iter().zip(plan) {
        if outcome.state != ItemState::NeedsReview {
            continue;
        }
        let Some(source) = outcome.analysis.source else {
            continue;
        };
        if SourceStamp::of(&outcome.input).ok() != Some(source) {
            continue;
        }
        let Ok(input) = sha256_file(&outcome.input) else {
            continue;
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
    Ok((!items.is_empty()).then(|| Session {
        version: FORMAT_VERSION,
        output: output.to_path_buf(),
        items,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;

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
