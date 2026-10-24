use std::collections::HashSet;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use cloakframe_core::coverage::{Problem, Resolution, has_unresolved};
use cloakframe_core::geometry::Rect;
use cloakframe_core::item::ItemId;
use cloakframe_core::review::{EditError, Edits, ManualTrack, PhotoEdits, VideoEdits};
use cloakframe_core::tracking::TrackId;
use serde::{Deserialize, Serialize};

use crate::fs_case::NameRule;
use crate::output_plan::free_name;
use crate::photo::format::ImageFormat;
use crate::publish::TEMP_PREFIX;
use crate::run::{Analysis, Decision, Fingerprint, JobItem, Start};
use crate::session::{Session, SessionItem, StoredMedia};
use crate::video::process::VideoAnalysis;

/// A reviewer's decisions on a stored session, as written by the desktop app or by hand for
/// `cloakframe-cli review`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewDecisions {
    pub items: Vec<ItemDecision>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "ItemDecisionForm", into = "ItemDecisionForm")]
pub struct ItemDecision {
    pub item: ItemId,
    /// The revision number the decision was made on; decisions on an older one are refused.
    pub revision: u64,
    pub action: Action,
    pub resolutions: Vec<EvidenceResolution>,
    /// The whole edit document for the item, the half of its kind; it replaces any earlier one.
    pub edits: Edits,
    /// A new file name in the same folder, for an output whose name is taken.
    pub rename_to: Option<String>,
}

/// The stored form of [`ItemDecision`], with the edits at the top level. serde cannot refuse
/// unknown keys next to a flattened field, and a misspelt edit key must not be ignored.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemDecisionForm {
    item: ItemId,
    revision: u64,
    action: Action,
    #[serde(default)]
    resolutions: Vec<EvidenceResolution>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    excluded: Vec<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    added: Vec<Rect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    excluded_tracks: Vec<TrackId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    added_tracks: Vec<ManualTrack>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rename_to: Option<String>,
}

impl From<ItemDecisionForm> for ItemDecision {
    fn from(f: ItemDecisionForm) -> Self {
        Self {
            item: f.item,
            revision: f.revision,
            action: f.action,
            resolutions: f.resolutions,
            edits: Edits {
                photo: PhotoEdits {
                    excluded: f.excluded,
                    added: f.added,
                },
                video: VideoEdits {
                    excluded_tracks: f.excluded_tracks,
                    added_tracks: f.added_tracks,
                },
            },
            rename_to: f.rename_to,
        }
    }
}

impl From<ItemDecision> for ItemDecisionForm {
    fn from(d: ItemDecision) -> Self {
        Self {
            item: d.item,
            revision: d.revision,
            action: d.action,
            resolutions: d.resolutions,
            excluded: d.edits.photo.excluded,
            added: d.edits.photo.added,
            excluded_tracks: d.edits.video.excluded_tracks,
            added_tracks: d.edits.video.added_tracks,
            rename_to: d.rename_to,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Approve,
    ExportWithWarnings,
    /// Re-encode under the metadata rules with nothing masked (D9).
    ExportWithoutAnonymization,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceResolution {
    /// Index into the item's stored evidence.
    pub evidence: usize,
    pub resolution: Resolution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    UnknownItem,
    DecidedTwice,
    Stale {
        current: u64,
    },
    SourceChanged,
    /// The item was analyzed with other models or settings than the ones rendering it now.
    SettingsChanged,
    NoSuchEvidence(usize),
    Edit(EditError),
    /// Evidence marked as covered by a manual mask that the added areas do not cover.
    NotCovered {
        evidence: usize,
    },
    /// Evidence with no area of its own marked as covered by a manual mask.
    NothingToCover {
        evidence: usize,
    },
    UnresolvedProblems,
    NameTaken {
        suggestion: PathBuf,
    },
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
            Self::SettingsChanged => f.write_str(
                "the models or settings differ from the ones the item was analyzed with; run it again",
            ),
            Self::NoSuchEvidence(i) => write!(f, "the item has no evidence #{i}"),
            Self::Edit(e) => e.fmt(f),
            Self::NotCovered { evidence } => write!(
                f,
                "evidence #{evidence} is marked as covered by a manual mask, but the added areas or tracks do not cover it"
            ),
            Self::NothingToCover { evidence } => write!(
                f,
                "evidence #{evidence} has no area to cover; dismiss it or export with warnings"
            ),
            Self::UnresolvedProblems => f.write_str(
                "unresolved detection problems remain; cover or dismiss them, or export with warnings",
            ),
            Self::NameTaken { suggestion } => {
                write!(f, "the output name is taken; free name: {}", suggestion.display())
            }
            Self::InvalidName => f.write_str(
                "rename_to must be a plain file name in the same folder, with an extension of the same kind and no characters or names Windows refuses",
            ),
        }
    }
}

#[derive(Debug, Default)]
pub struct Applied {
    pub jobs: Vec<JobItem>,
    pub rejected: Vec<(ItemId, Rejection)>,
}

/// Checks each decision against the session and the processor that will render it, and turns
/// the accepted ones into jobs. Accepted items have their resolutions, edits, name and revision
/// updated in `session`; rejected items are left as they were.
pub fn apply(
    session: &mut Session,
    decisions: &ReviewDecisions,
    fingerprint: impl Fn(&Path) -> Fingerprint,
) -> Applied {
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
        match check(
            stored,
            decision,
            fingerprint(&stored.input),
            &output,
            &claimed,
        ) {
            Ok(accepted) => {
                let folder = accepted.destination.parent().unwrap_or(Path::new(""));
                let case = NameRule::probe(&output.join(folder));
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
                            Action::ExportWithoutAnonymization => {
                                Decision::ExportWithoutAnonymization
                            }
                        },
                        analysis: Box::new(stored_analysis(stored)),
                        edits: stored_edits(stored),
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
    fingerprint: Fingerprint,
    output: &Path,
    claimed: &HashSet<(PathBuf, String)>,
) -> Result<SessionItem, Rejection> {
    if decision.revision != stored.revision.number() {
        return Err(Rejection::Stale {
            current: stored.revision.number(),
        });
    }
    let key = stored.revision.key();
    if (key.models, key.settings) != (fingerprint.models, fingerprint.settings) {
        return Err(Rejection::SettingsChanged);
    }
    if !stored.source_unchanged() {
        return Err(Rejection::SourceChanged);
    }
    let mut item = stored.clone();
    let wrong_media = Rejection::Edit(EditError::WrongMedia);
    match &mut item.media {
        StoredMedia::Photo { detections, edits } => {
            if !decision.edits.video.is_empty() {
                return Err(wrong_media);
            }
            let photo = &decision.edits.photo;
            photo.check(detections.len()).map_err(Rejection::Edit)?;
            edits.clone_from(photo);
        }
        StoredMedia::Video {
            frames,
            width,
            height,
            tracks,
            edits,
        } => {
            if !decision.edits.photo.is_empty() {
                return Err(wrong_media);
            }
            let video = &decision.edits.video;
            video
                .check(
                    |id| tracks.iter().any(|t| t.id == id),
                    *frames,
                    (*width, *height),
                )
                .map_err(Rejection::Edit)?;
            edits.clone_from(video);
        }
    }
    for r in &decision.resolutions {
        let evidence = item
            .evidence
            .get_mut(r.evidence)
            .ok_or(Rejection::NoSuchEvidence(r.evidence))?;
        if r.resolution == Resolution::CoveredByManualMask {
            // A weak candidate's box must lie in the added areas. A gap has no box, so a manual
            // track must show in each of its frames; where cannot be checked (M5 plan). Capped
            // candidates and failed frames can only be dismissed.
            let covered = match (&evidence.problem, &item.media) {
                (Problem::WeakCandidate { bbox, .. }, StoredMedia::Photo { edits, .. }) => {
                    bbox.is_covered_by(&edits.added)
                }
                (Problem::UnresolvedGap { first, last, .. }, StoredMedia::Video { edits, .. }) => {
                    edits.covers(*first, *last)
                }
                _ => {
                    return Err(Rejection::NothingToCover {
                        evidence: r.evidence,
                    });
                }
            };
            if !covered {
                return Err(Rejection::NotCovered {
                    evidence: r.evidence,
                });
            }
        }
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
        if !is_plain_name(name) || !keeps_the_kind(&item.destination, name) {
            return Err(Rejection::InvalidName);
        }
        item.destination = folder.join(name);
    }
    let case = NameRule::probe(&output.join(&folder));
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

/// What the item's render resumes from: its stored analysis.
fn stored_analysis(stored: &SessionItem) -> Analysis {
    let key = stored.revision.key();
    let (detections, video) = match &stored.media {
        StoredMedia::Photo { detections, .. } => (detections.clone(), None),
        StoredMedia::Video {
            frames,
            width,
            height,
            tracks,
            ..
        } => (
            Vec::new(),
            Some(VideoAnalysis {
                timeline: key.timeline,
                frames: *frames,
                size: (*width, *height),
                tracks: tracks.clone(),
            }),
        ),
    };
    Analysis {
        detections,
        evidence: stored.evidence.clone(),
        source: Some(stored.source),
        input: Some(key.input),
        video,
    }
}

fn stored_edits(stored: &SessionItem) -> Edits {
    match &stored.media {
        StoredMedia::Photo { edits, .. } => Edits {
            photo: edits.clone(),
            ..Edits::default()
        },
        StoredMedia::Video { edits, .. } => Edits {
            video: edits.clone(),
            ..Edits::default()
        },
    }
}

/// One name in the folder that every system can hold: output folders are often shared with
/// Windows, which refuses these characters and device names and drops a trailing dot or space.
fn is_plain_name(name: &str) -> bool {
    let mut components = Path::new(name).components();
    let stem = name.split('.').next().unwrap_or_default();
    let reserved = ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|r| stem.eq_ignore_ascii_case(r))
        || ["COM", "LPT"].iter().any(|r| {
            stem.len() == 4
                && stem.get(..3).is_some_and(|p| p.eq_ignore_ascii_case(r))
                && stem.as_bytes()[3].is_ascii_digit()
        });
    matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && !name.contains(['/', '\\', '<', '>', ':', '"', '|', '?', '*'])
        && !name.chars().any(char::is_control)
        && !name.ends_with(['.', ' '])
        && !reserved
        && !name.starts_with(TEMP_PREFIX)
}

/// The output format follows the name's extension, so a photo is renamed only to an image
/// extension and a video only to `.mp4`; otherwise the data would carry a name that says
/// otherwise, such as PNG data in `astro.txt`.
fn keeps_the_kind(destination: &Path, name: &str) -> bool {
    let is_image = |p: &Path| {
        p.extension()
            .and_then(|e| e.to_str())
            .and_then(ImageFormat::from_extension)
            .is_some()
    };
    let is_video = |p: &Path| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("mp4"));
    let new = Path::new(name);
    (is_image(destination) && is_image(new)) || (is_video(destination) && is_video(new))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::SourceStamp;
    use crate::output_plan::Conflict;
    use crate::session::FORMAT_VERSION;
    use crate::test_support::TestDir;
    use cloakframe_core::coverage::CoverageEvidence;
    use cloakframe_core::detection::{Detection, DetectionKind, ModelId};
    use cloakframe_core::review::{Digest, ReviewKey, ReviewRevision};

    const FINGERPRINT: Fingerprint = Fingerprint {
        models: Digest([2; 32]),
        settings: Digest([3; 32]),
    };

    fn same_settings(_: &Path) -> Fingerprint {
        FINGERPRINT
    }

    fn rect(x: f32) -> Rect {
        Rect::new(x, 0.0, 8.0, 8.0).unwrap()
    }

    fn gap() -> CoverageEvidence {
        CoverageEvidence::unresolved(Problem::UnresolvedGap {
            detection: DetectionKind::Face,
            tracks: vec![1],
            first: 30,
            last: 64,
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
                        media: StoredMedia::Photo {
                            detections: vec![
                                Detection::new(ModelId::Yolo5FaceN, 0.9, rect(0.0), None, None)
                                    .unwrap(),
                            ],
                            edits: PhotoEdits::default(),
                        },
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
            edits: Edits::default(),
            rename_to: None,
        }
    }

    fn weak(bbox: Rect) -> CoverageEvidence {
        CoverageEvidence::unresolved(Problem::WeakCandidate {
            model: ModelId::Yolo5FaceN,
            frame: None,
            bbox,
            score: 0.3,
        })
    }

    fn covered(mut d: ItemDecision, added: Vec<Rect>) -> ItemDecision {
        d.resolutions.push(EvidenceResolution {
            evidence: 0,
            resolution: Resolution::CoveredByManualMask,
        });
        d.edits.photo.added = added;
        d
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
            same_settings,
        );
        assert_eq!(
            applied.rejected,
            [(ItemId(0), Rejection::UnresolvedProblems)]
        );
        assert_eq!(applied.jobs.len(), 2);
        assert_eq!(s.items[0].revision.number(), 0);
        assert_eq!(s.items[1].revision.number(), 1);
        assert_eq!(s.items[1].evidence[0].resolution, Resolution::Dismissed);
        let Start::Reviewed {
            decision, analysis, ..
        } = &applied.jobs[1].start
        else {
            panic!()
        };
        assert_eq!(*decision, Decision::ExportWithWarnings);
        assert_eq!(analysis.evidence, [gap()]);
        assert_eq!(analysis.source, Some(s.items[2].source));
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
            same_settings,
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
            same_settings,
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
        // Also refused: names whose extension the output format would not follow (the data would
        // be written under a name that says otherwise) and names Windows cannot hold.
        for bad in [
            "../a.jpg",
            "sub/a.jpg",
            "",
            ".cloakframe-tmp-x.jpg",
            "astro.txt",
            "a",
            "a.mp4",
            "a.jpg:hidden",
            "a?.jpg",
            "CON.jpg",
            "lpt1.png",
            "a.jpg.",
            "a.jpg ",
        ] {
            let mut d = export();
            d.rename_to = Some(bad.into());
            let applied = apply(&mut s, &ReviewDecisions { items: vec![d] }, same_settings);
            assert_eq!(applied.rejected[0].1, Rejection::InvalidName, "{bad:?}");
        }
        // A four-byte stem that is not a device name, another image format, a name like one.
        for good in ["a\u{c0ac}.jpg", "a (2).png", "COM.jpg", "console.jpg"] {
            assert!(
                is_plain_name(good) && keeps_the_kind(Path::new("a.jpg"), good),
                "{good}"
            );
        }
        let mut renamed = export();
        renamed.rename_to = Some("a (2).jpg".into());
        let applied = apply(
            &mut s,
            &ReviewDecisions {
                items: vec![renamed],
            },
            same_settings,
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
            same_settings,
        );
        assert_eq!(applied.jobs.len(), 1);
        assert!(matches!(
            applied.rejected[0],
            (ItemId(1), Rejection::NameTaken { .. })
        ));
    }

    #[test]
    fn edits_are_checked_stored_and_handed_to_the_job() {
        let dir = TestDir::new();
        let mut s = session(&dir, &["a.jpg"]);
        let mut bad = decide(0, Action::ExportWithWarnings);
        bad.edits.photo.excluded = vec![1];
        let mut empty_area = decide(0, Action::ExportWithWarnings);
        empty_area.edits.photo.added = vec![Rect::new(0.0, 0.0, 0.0, 4.0).unwrap()];
        for (d, want) in [
            (bad, Rejection::Edit(EditError::NoSuchDetection(1))),
            (empty_area, Rejection::Edit(EditError::EmptyArea(0))),
        ] {
            let applied = apply(&mut s, &ReviewDecisions { items: vec![d] }, same_settings);
            assert_eq!(applied.rejected, [(ItemId(0), want)]);
        }

        s.items[0].evidence = vec![weak(rect(20.0))];
        let mut covered = covered(decide(0, Action::Approve), vec![rect(20.0)]);
        covered.edits.photo.excluded = vec![0];
        let applied = apply(
            &mut s,
            &ReviewDecisions {
                items: vec![covered.clone()],
            },
            same_settings,
        );
        assert!(applied.rejected.is_empty());
        let StoredMedia::Photo { edits, .. } = &s.items[0].media else {
            panic!()
        };
        assert_eq!(*edits, covered.edits.photo);
        let Start::Reviewed { edits, .. } = &applied.jobs[0].start else {
            panic!()
        };
        assert_eq!(*edits, covered.edits);
    }

    #[test]
    fn a_manual_mask_resolves_only_the_area_it_covers() {
        let dir = TestDir::new();
        let mut s = session(&dir, &["a.jpg"]);
        let capped = CoverageEvidence::unresolved(Problem::CappedCandidates {
            model: ModelId::Yolo5FaceN,
            frame: None,
            kept: 1,
            found: 3,
        });
        let pixel = Rect::new(0.0, 0.0, 1.0, 1.0).unwrap();
        let half = Rect::new(40.0, 0.0, 4.0, 8.0).unwrap();
        for (evidence, added, want) in [
            (
                weak(rect(40.0)),
                vec![],
                Rejection::NotCovered { evidence: 0 },
            ),
            (
                weak(rect(40.0)),
                vec![pixel],
                Rejection::NotCovered { evidence: 0 },
            ),
            (
                weak(rect(40.0)),
                vec![half],
                Rejection::NotCovered { evidence: 0 },
            ),
            // Nothing to compare an area with: these can only be dismissed.
            (
                capped,
                vec![pixel],
                Rejection::NothingToCover { evidence: 0 },
            ),
            (
                gap(),
                vec![pixel],
                Rejection::NothingToCover { evidence: 0 },
            ),
        ] {
            s.items[0].evidence = vec![evidence];
            let d = covered(decide(0, Action::Approve), added);
            let applied = apply(&mut s, &ReviewDecisions { items: vec![d] }, same_settings);
            assert_eq!(applied.rejected, [(ItemId(0), want)]);
        }

        s.items[0].evidence = vec![weak(rect(40.0))];
        let other_half = Rect::new(44.0, 0.0, 4.0, 8.0).unwrap();
        let d = covered(decide(0, Action::Approve), vec![half, other_half]);
        let applied = apply(&mut s, &ReviewDecisions { items: vec![d] }, same_settings);
        assert!(applied.rejected.is_empty(), "{:?}", applied.rejected);
    }

    #[test]
    fn decisions_for_other_models_or_settings_are_refused() {
        let dir = TestDir::new();
        let mut s = session(&dir, &["a.jpg"]);
        for fingerprint in [
            Fingerprint {
                models: Digest([9; 32]),
                ..FINGERPRINT
            },
            Fingerprint {
                settings: Digest([9; 32]),
                ..FINGERPRINT
            },
        ] {
            let applied = apply(
                &mut s,
                &ReviewDecisions {
                    items: vec![decide(0, Action::ExportWithoutAnonymization)],
                },
                |_: &Path| fingerprint,
            );
            assert_eq!(applied.rejected, [(ItemId(0), Rejection::SettingsChanged)]);
        }
        let applied = apply(
            &mut s,
            &ReviewDecisions {
                items: vec![decide(0, Action::ExportWithoutAnonymization)],
            },
            same_settings,
        );
        let Start::Reviewed { decision, .. } = &applied.jobs[0].start else {
            panic!()
        };
        assert_eq!(*decision, Decision::ExportWithoutAnonymization);
    }

    fn video_item(session: &mut Session) {
        let item = &mut session.items[0];
        item.destination = PathBuf::from("clip.mp4");
        item.media = StoredMedia::Video {
            frames: 90,
            width: 640,
            height: 360,
            tracks: vec![crate::video::tracking::TrackSummary {
                id: 1,
                kind: DetectionKind::Face,
                first: 0,
                last: 89,
                detections: 80,
                strong: 80,
                low_confidence: false,
            }],
            edits: VideoEdits::default(),
        };
    }

    fn manual(first: u64, last: u64) -> ManualTrack {
        ManualTrack {
            first,
            last,
            keyframes: vec![cloakframe_core::review::Keyframe {
                frame: first,
                bbox: rect(0.0),
            }],
        }
    }

    /// A video's decision names its tracks and adds tracks; a manual track resolves a gap only
    /// over every frame of it. Photo edits on a video, and video edits on a photo, are refused.
    #[test]
    fn video_decisions_edit_tracks_and_cover_gaps_with_manual_tracks() {
        let dir = TestDir::new();
        let mut s = session(&dir, &["clip.jpg"]);
        video_item(&mut s);
        let decide_video = |video: VideoEdits| {
            let mut d = covered(decide(0, Action::Approve), Vec::new());
            d.edits.video = video;
            d
        };
        let video = |excluded: Vec<TrackId>, added: Vec<ManualTrack>| VideoEdits {
            excluded_tracks: excluded,
            added_tracks: added,
        };
        let photo_on_video = covered(decide(0, Action::Approve), vec![rect(0.0)]);
        for (d, want) in [
            (photo_on_video, Rejection::Edit(EditError::WrongMedia)),
            (
                decide_video(video(vec![2], Vec::new())),
                Rejection::Edit(EditError::NoSuchTrack(2)),
            ),
            (
                decide_video(video(Vec::new(), vec![manual(80, 90)])),
                Rejection::Edit(EditError::ManualTrack {
                    track: 0,
                    error: cloakframe_core::review::ManualTrackError::PastTheEnd,
                }),
            ),
            (
                decide_video(video(Vec::new(), vec![manual(30, 40), manual(42, 64)])),
                Rejection::NotCovered { evidence: 0 },
            ),
        ] {
            let applied = apply(&mut s, &ReviewDecisions { items: vec![d] }, same_settings);
            assert_eq!(applied.rejected, [(ItemId(0), want)]);
        }

        let edits = video(vec![1], vec![manual(30, 41), manual(42, 64)]);
        let applied = apply(
            &mut s,
            &ReviewDecisions {
                items: vec![decide_video(edits.clone())],
            },
            same_settings,
        );
        assert!(applied.rejected.is_empty(), "{:?}", applied.rejected);
        let StoredMedia::Video { edits: stored, .. } = &s.items[0].media else {
            panic!()
        };
        assert_eq!(*stored, edits);
        let Start::Reviewed {
            analysis,
            edits: job_edits,
            ..
        } = &applied.jobs[0].start
        else {
            panic!()
        };
        assert_eq!(job_edits.video, edits);
        let video = analysis.video.as_ref().unwrap();
        assert_eq!(
            (video.timeline, video.frames, video.size),
            (Digest([4; 32]), 90, (640, 360))
        );
        assert_eq!(video.tracks[0].id, 1);

        let mut photo = session(&dir, &["a.jpg"]);
        let mut d = decide(0, Action::ExportWithWarnings);
        d.edits.video.excluded_tracks = vec![1];
        let applied = apply(
            &mut photo,
            &ReviewDecisions { items: vec![d] },
            same_settings,
        );
        assert_eq!(
            applied.rejected,
            [(ItemId(0), Rejection::Edit(EditError::WrongMedia))]
        );
    }

    #[test]
    fn the_decision_file_format() {
        let decisions: ReviewDecisions = serde_json::from_str(
            r#"{"items": [
                {"item": 0, "revision": 0, "action": "approve",
                 "resolutions": [{"evidence": 0, "resolution": "covered_by_manual_mask"}]},
                {"item": 1, "revision": 2, "action": "export_with_warnings", "rename_to": "b (2).jpg"},
                {"item": 2, "revision": 0, "action": "export_without_anonymization",
                 "excluded": [0], "added": [{"x": 1, "y": 2, "width": 3, "height": 4}]},
                {"item": 3, "revision": 0, "action": "approve", "excluded_tracks": [4],
                 "added_tracks": [{"first": 2, "last": 8, "keyframes": [
                    {"frame": 3, "bbox": {"x": 10, "y": 20, "width": 30, "height": 40}}]}]}
            ]}"#,
        )
        .unwrap();
        assert_eq!(
            decisions.items[2].action,
            Action::ExportWithoutAnonymization
        );
        assert_eq!(decisions.items[2].edits.photo.excluded, [0]);
        assert_eq!(decisions.items[2].edits.photo.added[0].height(), 4.0);
        assert_eq!(decisions.items[1].edits, Edits::default());
        assert_eq!(
            decisions.items[0].resolutions[0].resolution,
            Resolution::CoveredByManualMask
        );
        assert_eq!(decisions.items[1].rename_to.as_deref(), Some("b (2).jpg"));
        let video = &decisions.items[3].edits.video;
        assert_eq!(video.excluded_tracks, [4]);
        assert_eq!(video.added_tracks[0].keyframes[0].bbox.width(), 30.0);
        assert_eq!(
            (video.added_tracks[0].first, video.added_tracks[0].last),
            (2, 8)
        );
    }

    #[test]
    fn unknown_keys_are_refused() {
        // A misspelt "added" or "resolutions" would publish without the mask the reviewer meant.
        for json in [
            r#"{"items": [{"item": 0, "revision": 0, "action": "approve",
                "add": [{"x": 1, "y": 2, "width": 3, "height": 4}]}]}"#,
            r#"{"items": [{"item": 0, "revision": 0, "action": "approve",
                "resolution": [{"evidence": 0, "resolution": "dismissed"}]}]}"#,
            r#"{"items": [{"item": 0, "revision": 0, "action": "approve",
                "resolutions": [{"evidence": 0, "resolution": "dismissed", "note": "x"}]}]}"#,
            r#"{"items": [], "item": []}"#,
            r#"{"items": [{"item": 0, "revision": 0, "action": "approve",
                "added_tracks": [{"first": 0, "last": 1, "keyframe": []}]}]}"#,
            r#"{"items": [{"item": 0, "revision": 0, "action": "approve",
                "added_tracks": [{"first": 0, "last": 1, "keyframes": [
                    {"frame": 0, "box": {"x": 1, "y": 2, "width": 3, "height": 4}}]}]}]}"#,
        ] {
            let error = serde_json::from_str::<ReviewDecisions>(json).unwrap_err();
            assert!(
                error.to_string().contains("unknown field"),
                "{json}: {error}"
            );
        }
    }
}
