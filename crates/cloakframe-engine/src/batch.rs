use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use cloakframe_core::item::{ItemId, ItemState, ReviewMode};
use cloakframe_core::review::{Digest, PhotoEdits, ReviewKey, ReviewRevision, VideoEdits};
use sha2::{Digest as _, Sha256};

use crate::identity::SourceStamp;
use crate::output_plan::{PlannedOutput, plan_outputs};
use crate::publish::remove_stale_temp_files;
use crate::run::{Fingerprint, Job, JobItem, RunReport, Start};
use crate::scan::{MediaKind, ScannedFile};
use crate::session::{FORMAT_VERSION, Session, SessionId, SessionItem, SessionStore, StoredMedia};

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

/// Removes the temp outputs earlier runs left in the job's output folders, before this run
/// writes its own there.
pub fn remove_stale_outputs(job: &Job) {
    let mut folders: Vec<PathBuf> = job
        .items
        .iter()
        .filter_map(|i| i.destination.parent().map(PathBuf::from))
        .collect();
    folders.sort();
    folders.dedup();
    for folder in folders {
        let _ = remove_stale_temp_files(&folder, SystemTime::now());
    }
}

/// What [`keep_for_review`] did with a finished run.
pub struct Kept {
    /// Whether a session was saved, and how saving went.
    pub saved: Option<io::Result<()>>,
    /// Removing the records no review needs: all of them when nothing was saved.
    pub cleaned: io::Result<()>,
    pub left_out: Vec<(ItemId, LeftOut)>,
}

/// Saves as session `id` the items `report` left for review, with the front end's `options`
/// ([`Session::options`]), and removes the video records of the rest. Done before anything is
/// shown, which can fail once a terminal is gone.
pub fn keep_for_review(
    store: &SessionStore,
    id: &SessionId,
    report: &RunReport,
    plan: &[PlannedOutput],
    output: &Path,
    fingerprint: impl Fn(&Path) -> Fingerprint,
    options: Option<serde_json::Value>,
) -> Kept {
    let (mut session, left_out) = session_from_report(report, plan, output, fingerprint);
    if let Some(session) = &mut session {
        session.options = options;
    }
    let saved = session.as_ref().map(|session| store.save(id, session));
    let cleaned = match (&session, &saved) {
        (Some(session), Some(Ok(()))) => store.prune(id, session),
        _ => store.remove(id),
    };
    Kept {
        saved,
        cleaned,
        left_out,
    }
}

/// Why an item the run held for review is not in its review session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeftOut {
    /// The run kept no record of the source it analyzed.
    NotAnalyzed,
    /// The source changed or vanished since it was read for analysis.
    SourceChanged,
}

impl fmt::Display for LeftOut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnalyzed => f.write_str("the item was not analyzed; run it again"),
            Self::SourceChanged => {
                f.write_str("the source changed or is gone since analysis; run it again")
            }
        }
    }
}

/// The review session for the items a run left in `NeedsReview` (`None` if there are none), and
/// the held items that cannot be reviewed, which need a new run.
pub fn session_from_report(
    report: &RunReport,
    plan: &[PlannedOutput],
    output: &Path,
    fingerprint: impl Fn(&Path) -> Fingerprint,
) -> (Option<Session>, Vec<(ItemId, LeftOut)>) {
    let mut items = Vec::new();
    let mut left_out = Vec::new();
    for (outcome, planned) in report.items.iter().zip(plan) {
        if outcome.state != ItemState::NeedsReview {
            continue;
        }
        let analysis = &outcome.analysis;
        let is_video = MediaKind::of(&outcome.input) == Some(MediaKind::Video);
        let (Some(source), Some(input), true) = (
            analysis.source,
            analysis.input,
            analysis.video.is_some() == is_video,
        ) else {
            left_out.push((outcome.item, LeftOut::NotAnalyzed));
            continue;
        };
        if SourceStamp::of(&outcome.input).ok() != Some(source) {
            left_out.push((outcome.item, LeftOut::SourceChanged));
            continue;
        }
        let (media, timeline) = match &analysis.video {
            Some(video) => (
                StoredMedia::Video {
                    frames: video.frames,
                    width: video.size.0,
                    height: video.size.1,
                    seconds: video.seconds,
                    tracks: video.tracks.clone(),
                    edits: VideoEdits::default(),
                },
                video.timeline,
            ),
            // Photos have no timeline.
            None => (
                StoredMedia::Photo {
                    detections: analysis.detections.clone(),
                    edits: PhotoEdits::default(),
                },
                Digest(Sha256::digest(b"").into()),
            ),
        };
        let fingerprint = fingerprint(&outcome.input);
        let key = ReviewKey {
            input,
            models: fingerprint.models,
            settings: fingerprint.settings,
            timeline,
        };
        items.push(SessionItem {
            item: outcome.item,
            input: outcome.input.clone(),
            destination: planned.relative.clone(),
            conflict: planned.conflict,
            suggestion: planned.suggestion.clone(),
            source,
            revision: ReviewRevision::first(key),
            media,
            evidence: analysis.evidence.clone(),
        });
    }
    let session = (!items.is_empty()).then(|| Session {
        version: FORMAT_VERSION,
        output: output.to_path_buf(),
        items,
        options: None,
    });
    (session, left_out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;

    /// An item held for review whose source changed or vanished cannot be reviewed, nor can a
    /// video without its analysis; each is reported with the reason instead of silently missing
    /// from the session. A video is kept with its timeline, tracks and its pipeline's settings.
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
                outcome(4, "held.mp4"),
            ],
        };
        let mut report = report;
        let track = crate::video::tracking::TrackSummary {
            id: 1,
            kind: cloakframe_core::detection::DetectionKind::Face,
            first: 0,
            last: 9,
            detections: 10,
            strong: 10,
            low_confidence: false,
        };
        report.items[4].analysis.video = Some(crate::video::process::VideoAnalysis {
            timeline: Digest([7; 32]),
            frames: 10,
            size: (64, 48),
            seconds: 0.4,
            tracks: vec![track],
        });
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
        let fingerprint = |input: &Path| Fingerprint {
            models: Digest([0; 32]),
            settings: Digest([u8::from(MediaKind::of(input) == Some(MediaKind::Video)); 32]),
        };
        let (session, left_out) =
            session_from_report(&report, &plan, &dir.path().join("out"), fingerprint);
        let session = session.unwrap();
        let kept: Vec<ItemId> = session.items.iter().map(|i| i.item).collect();
        assert_eq!(kept, [ItemId(0), ItemId(4)]);
        assert_eq!(
            left_out,
            [
                (ItemId(1), LeftOut::SourceChanged),
                (ItemId(2), LeftOut::SourceChanged),
                (ItemId(3), LeftOut::NotAnalyzed)
            ]
        );
        let video = &session.items[1];
        let key = video.revision.key();
        assert_eq!(
            (key.timeline, key.settings),
            (Digest([7; 32]), Digest([1; 32]))
        );
        assert_eq!(session.items[0].revision.key().settings, Digest([0; 32]));
        assert_eq!(
            video.media,
            StoredMedia::Video {
                frames: 10,
                width: 64,
                height: 48,
                seconds: 0.4,
                tracks: vec![track],
                edits: VideoEdits::default(),
            }
        );
    }
}
