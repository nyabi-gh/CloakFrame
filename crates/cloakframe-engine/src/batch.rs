use std::fmt;
use std::path::Path;

use cloakframe_core::item::{ItemId, ItemState, ReviewMode};
use cloakframe_core::review::{Digest, PhotoEdits, ReviewKey, ReviewRevision};
use sha2::{Digest as _, Sha256};

use crate::identity::SourceStamp;
use crate::output_plan::{PlannedOutput, plan_outputs};
use crate::run::{Fingerprint, Job, JobItem, RunReport, Start};
use crate::scan::{MediaKind, ScannedFile};
use crate::session::{FORMAT_VERSION, Session, SessionItem};

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
    /// Videos are not kept for review yet: their records and timeline go into a session with
    /// M5's review stage.
    Video,
}

impl fmt::Display for LeftOut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnalyzed => f.write_str("the item was not analyzed; run it again"),
            Self::SourceChanged => {
                f.write_str("the source changed or is gone since analysis; run it again")
            }
            Self::Video => f.write_str("videos cannot be reviewed yet"),
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
        if MediaKind::of(&outcome.input) == Some(MediaKind::Video) {
            left_out.push((outcome.item, LeftOut::Video));
            continue;
        }
        let (Some(source), Some(input)) = (outcome.analysis.source, outcome.analysis.input) else {
            left_out.push((outcome.item, LeftOut::NotAnalyzed));
            continue;
        };
        if SourceStamp::of(&outcome.input).ok() != Some(source) {
            left_out.push((outcome.item, LeftOut::SourceChanged));
            continue;
        }
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
    use crate::test_support::TestDir;

    /// An item held for review whose source changed or vanished cannot be reviewed, nor can a
    /// video yet; each is reported with the reason instead of silently missing from the session.
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
                    input: Some(Digest([1; 32])),
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
                outcome(3, "clip.MP4"),
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
                (ItemId(2), LeftOut::SourceChanged),
                (ItemId(3), LeftOut::Video)
            ]
        );
    }
}
