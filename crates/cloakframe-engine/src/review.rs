use std::collections::HashSet;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use cloakframe_core::coverage::{Resolution, has_unresolved};
use cloakframe_core::item::ItemId;
use serde::{Deserialize, Serialize};

use crate::fs_case::NameCase;
use crate::output_plan::free_name;
use crate::publish::TEMP_PREFIX;
use crate::run::{Analysis, Decision, JobItem, Start};
use crate::session::{Session, SessionItem};

/// A reviewer's decisions on a stored session, as written by the desktop app or by hand for
/// `cloakframe-cli review`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewDecisions {
    pub items: Vec<ItemDecision>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemDecision {
    pub item: ItemId,
    /// The revision number the decision was made on; decisions on an older one are refused.
    pub revision: u64,
    pub action: Action,
    #[serde(default)]
    pub resolutions: Vec<EvidenceResolution>,
    /// A new file name in the same folder, for an output whose name is taken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rename_to: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Approve,
    ExportWithWarnings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceResolution {
    /// Index into the item's stored evidence.
    pub evidence: usize,
    pub resolution: Resolution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    UnknownItem,
    DecidedTwice,
    Stale { current: u64 },
    SourceChanged,
    NoSuchEvidence(usize),
    UnresolvedProblems,
    NameTaken { suggestion: PathBuf },
    InvalidName,
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownItem => f.write_str("the session has no such item"),
            Self::DecidedTwice => f.write_str("the item is decided more than once"),
            Self::Stale { current } => {
                write!(f, "the decision is for an older revision (current: {current})")
            }
            Self::SourceChanged => f.write_str("the source file changed or is gone since analysis"),
            Self::NoSuchEvidence(i) => write!(f, "the item has no evidence #{i}"),
            Self::UnresolvedProblems => f.write_str(
                "unresolved detection problems remain; cover or dismiss them, or export with warnings",
            ),
            Self::NameTaken { suggestion } => {
                write!(f, "the output name is taken; free name: {}", suggestion.display())
            }
            Self::InvalidName => {
                f.write_str("rename_to must be a plain file name in the same folder")
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct Applied {
    pub jobs: Vec<JobItem>,
    pub rejected: Vec<(ItemId, Rejection)>,
}

/// Checks each decision against the session and turns the accepted ones into jobs. Accepted
/// items have their resolutions, name and revision updated in `session`; rejected items are left
/// as they were.
pub fn apply(session: &mut Session, decisions: &ReviewDecisions) -> Applied {
    let mut applied = Applied::default();
    let mut seen = HashSet::new();
    let mut claimed: HashSet<(PathBuf, String)> = HashSet::new();
    for decision in &decisions.items {
        if !seen.insert(decision.item) {
            applied
                .rejected
                .push((decision.item, Rejection::DecidedTwice));
            continue;
        }
        let output = session.output.clone();
        let Some(stored) = session.items.iter_mut().find(|i| i.item == decision.item) else {
            applied
                .rejected
                .push((decision.item, Rejection::UnknownItem));
            continue;
        };
        match check(stored, decision, &output, &claimed) {
            Ok(accepted) => {
                let folder = accepted.destination.parent().unwrap_or(Path::new(""));
                let case = NameCase::probe(&output.join(folder));
                let name = accepted
                    .destination
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                claimed.insert((folder.to_path_buf(), case.key(&name)));
                *stored = accepted.clone();
                stored.revision = stored.revision.next();
                applied.jobs.push(JobItem {
                    id: stored.item,
                    input: stored.input.clone(),
                    destination: output.join(&stored.destination),
                    start: Start::Reviewed {
                        decision: match decision.action {
                            Action::Approve => Decision::Approve,
                            Action::ExportWithWarnings => Decision::ExportWithWarnings,
                        },
                        analysis: Analysis {
                            detections: stored.detections.clone(),
                            evidence: stored.evidence.clone(),
                        },
                    },
                });
            }
            Err(rejection) => applied.rejected.push((decision.item, rejection)),
        }
    }
    applied
}

fn check(
    stored: &SessionItem,
    decision: &ItemDecision,
    output: &Path,
    claimed: &HashSet<(PathBuf, String)>,
) -> Result<SessionItem, Rejection> {
    if decision.revision != stored.revision.number() {
        return Err(Rejection::Stale {
            current: stored.revision.number(),
        });
    }
    if !stored.source_unchanged() {
        return Err(Rejection::SourceChanged);
    }
    let mut item = stored.clone();
    for r in &decision.resolutions {
        let evidence = item
            .evidence
            .get_mut(r.evidence)
            .ok_or(Rejection::NoSuchEvidence(r.evidence))?;
        evidence.resolution = r.resolution;
    }
    if decision.action == Action::Approve && has_unresolved(&item.evidence) {
        return Err(Rejection::UnresolvedProblems);
    }

    let folder = item
        .destination
        .parent()
        .unwrap_or(Path::new(""))
        .to_path_buf();
    if let Some(name) = &decision.rename_to {
        if !is_plain_name(name) {
            return Err(Rejection::InvalidName);
        }
        item.destination = folder.join(name);
    }
    let case = NameCase::probe(&output.join(&folder));
    let is_free = |name: &str| {
        !claimed.contains(&(folder.clone(), case.key(name)))
            && output.join(&folder).join(name).symlink_metadata().is_err()
    };
    let name = item
        .destination
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !is_free(&name) {
        return Err(Rejection::NameTaken {
            suggestion: folder.join(free_name(&name, is_free)),
        });
    }
    item.conflict = None;
    item.suggestion = None;
    Ok(item)
}

fn is_plain_name(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && !name.contains(['/', '\\'])
        && !name.starts_with(TEMP_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::SourceStamp;
    use crate::output_plan::Conflict;
    use crate::session::FORMAT_VERSION;
    use crate::testdir::TestDir;
    use cloakframe_core::coverage::{CoverageEvidence, Problem};
    use cloakframe_core::review::{Digest, ReviewKey, ReviewRevision};

    fn gap() -> CoverageEvidence {
        CoverageEvidence::unresolved(Problem::UnresolvedGap {
            track: 1,
            first: 30,
            last: 54,
        })
    }

    fn session(dir: &TestDir, names: &[&str]) -> Session {
        let key = ReviewKey {
            input: Digest([1; 32]),
            models: Digest([2; 32]),
            settings: Digest([3; 32]),
            timeline: Digest([4; 32]),
        };
        Session {
            version: FORMAT_VERSION,
            output: dir.mkdir("out"),
            items: names
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    let input = dir.write(&format!("in/{name}"));
                    SessionItem {
                        item: ItemId(i as u64),
                        destination: PathBuf::from(name),
                        conflict: None,
                        suggestion: None,
                        source: SourceStamp::of(&input).unwrap(),
                        input,
                        revision: ReviewRevision::first(key),
                        detections: Vec::new(),
                        evidence: vec![gap()],
                    }
                })
                .collect(),
        }
    }

    fn decide(item: u64, action: Action) -> ItemDecision {
        ItemDecision {
            item: ItemId(item),
            revision: 0,
            action,
            resolutions: Vec::new(),
            rename_to: None,
        }
    }

    fn dismissed(mut d: ItemDecision) -> ItemDecision {
        d.resolutions.push(EvidenceResolution {
            evidence: 0,
            resolution: Resolution::Dismissed,
        });
        d
    }

    #[test]
    fn approval_needs_every_problem_resolved() {
        let dir = TestDir::new();
        let mut s = session(&dir, &["a.jpg", "b.jpg", "c.jpg"]);
        let applied = apply(
            &mut s,
            &ReviewDecisions {
                items: vec![
                    decide(0, Action::Approve),
                    dismissed(decide(1, Action::Approve)),
                    decide(2, Action::ExportWithWarnings),
                ],
            },
        );
        assert_eq!(
            applied.rejected,
            [(ItemId(0), Rejection::UnresolvedProblems)]
        );
        assert_eq!(applied.jobs.len(), 2);
        assert_eq!(s.items[0].revision.number(), 0);
        assert_eq!(s.items[1].revision.number(), 1);
        assert_eq!(s.items[1].evidence[0].resolution, Resolution::Dismissed);
        let Start::Reviewed { decision, analysis } = &applied.jobs[1].start else {
            panic!()
        };
        assert_eq!(*decision, Decision::ExportWithWarnings);
        assert_eq!(analysis.evidence, [gap()]);
    }

    #[test]
    fn refuses_stale_unknown_repeated_and_changed_items() {
        let dir = TestDir::new();
        let mut s = session(&dir, &["a.jpg", "b.jpg"]);
        std::fs::write(&s.items[1].input, b"edited").unwrap();
        let mut stale = decide(0, Action::ExportWithWarnings);
        stale.revision = 7;
        let applied = apply(
            &mut s,
            &ReviewDecisions {
                items: vec![
                    stale,
                    decide(0, Action::ExportWithWarnings),
                    decide(1, Action::ExportWithWarnings),
                    decide(9, Action::ExportWithWarnings),
                ],
            },
        );
        assert_eq!(
            applied.rejected,
            [
                (ItemId(0), Rejection::Stale { current: 0 }),
                (ItemId(0), Rejection::DecidedTwice),
                (ItemId(1), Rejection::SourceChanged),
                (ItemId(9), Rejection::UnknownItem),
            ]
        );
        assert!(applied.jobs.is_empty());
    }

    #[test]
    fn a_taken_name_needs_a_free_rename() {
        let dir = TestDir::new();
        let mut s = session(&dir, &["a.jpg"]);
        s.items[0].conflict = Some(Conflict::Existing);
        dir.write("out/a.jpg");
        let export = || decide(0, Action::ExportWithWarnings);
        let applied = apply(
            &mut s,
            &ReviewDecisions {
                items: vec![export()],
            },
        );
        assert_eq!(
            applied.rejected,
            [(
                ItemId(0),
                Rejection::NameTaken {
                    suggestion: "a (2).jpg".into()
                }
            )]
        );
        for bad in ["../a.jpg", "sub/a.jpg", "", ".cloakframe-tmp-x.jpg"] {
            let mut d = export();
            d.rename_to = Some(bad.into());
            let applied = apply(&mut s, &ReviewDecisions { items: vec![d] });
            assert_eq!(applied.rejected[0].1, Rejection::InvalidName, "{bad:?}");
        }
        let mut renamed = export();
        renamed.rename_to = Some("a (2).jpg".into());
        let applied = apply(
            &mut s,
            &ReviewDecisions {
                items: vec![renamed],
            },
        );
        assert!(applied.rejected.is_empty());
        assert_eq!(applied.jobs[0].destination, s.output.join("a (2).jpg"));
        assert_eq!(s.items[0].conflict, None);
    }

    #[test]
    fn two_decisions_cannot_claim_the_same_name() {
        let dir = TestDir::new();
        let mut s = session(&dir, &["a.jpg", "b.jpg"]);
        let mut second = decide(1, Action::ExportWithWarnings);
        second.rename_to = Some("a.jpg".into());
        let applied = apply(
            &mut s,
            &ReviewDecisions {
                items: vec![decide(0, Action::ExportWithWarnings), second],
            },
        );
        assert_eq!(applied.jobs.len(), 1);
        assert!(matches!(
            applied.rejected[0],
            (ItemId(1), Rejection::NameTaken { .. })
        ));
    }

    #[test]
    fn the_decision_file_format() {
        let decisions: ReviewDecisions = serde_json::from_str(
            r#"{"items": [
                {"item": 0, "revision": 0, "action": "approve",
                 "resolutions": [{"evidence": 0, "resolution": "covered_by_manual_mask"}]},
                {"item": 1, "revision": 2, "action": "export_with_warnings", "rename_to": "b (2).jpg"}
            ]}"#,
        )
        .unwrap();
        assert_eq!(
            decisions.items[0].resolutions[0].resolution,
            Resolution::CoveredByManualMask
        );
        assert_eq!(decisions.items[1].rename_to.as_deref(), Some("b (2).jpg"));
    }
}
