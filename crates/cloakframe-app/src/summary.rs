//! A run's result in one sentence, like "12 saved, 2 to review — faces missed for 1.2 s in one
//! video" (plan 6), and each item's problems in words.

use cloakframe_core::coverage::{CoverageEvidence, Problem};
use cloakframe_core::detection::DetectionKind;
use cloakframe_core::item::ItemState;
use fluent_bundle::FluentValue;

use crate::i18n::Translator;

/// One item as the summary needs it.
pub struct Item<'a> {
    pub state: ItemState,
    pub evidence: &'a [CoverageEvidence],
    /// A video's source frames and length, which give its gaps in seconds.
    pub video: Option<(u64, f64)>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Summary {
    pub saved: usize,
    pub review: usize,
    pub failed: usize,
    pub cancelled: usize,
    /// Videos with an unresolved gap of faces and their gaps' length in all, then plates'.
    pub missed: [(usize, f64); 2],
    pub capped: usize,
    pub weak: usize,
    pub failed_frames: usize,
}

/// A gap's length from its frames, at the video's average frame length.
pub fn gap_seconds(first: u64, last: u64, video: Option<(u64, f64)>) -> Option<f64> {
    let (frames, seconds) = video.filter(|&(frames, _)| frames > 0)?;
    Some((last.saturating_sub(first) + 1) as f64 * seconds / frames as f64)
}

fn kind_index(kind: DetectionKind) -> usize {
    match kind {
        DetectionKind::Face => 0,
        DetectionKind::Plate => 1,
    }
}

pub fn summarize<'a>(items: impl IntoIterator<Item = Item<'a>>) -> Summary {
    let mut summary = Summary::default();
    for item in items {
        match item.state {
            ItemState::Published(_) => summary.saved += 1,
            ItemState::NeedsReview => summary.review += 1,
            ItemState::Failed => summary.failed += 1,
            ItemState::Cancelled => summary.cancelled += 1,
            _ => {}
        }
        if item.state != ItemState::NeedsReview {
            continue;
        }
        let (mut missed, mut capped, mut weak, mut failed_frames) = ([0.0; 2], false, false, false);
        let mut has_gap = [false; 2];
        for e in item.evidence.iter().filter(|e| !e.is_resolved()) {
            match &e.problem {
                Problem::UnresolvedGap {
                    detection,
                    first,
                    last,
                    ..
                } => {
                    let k = kind_index(*detection);
                    has_gap[k] = true;
                    missed[k] += gap_seconds(*first, *last, item.video).unwrap_or(0.0);
                }
                Problem::CappedCandidates { .. } => capped = true,
                Problem::WeakCandidate { .. } => weak = true,
                Problem::FailedFrame { .. } => failed_frames = true,
            }
        }
        for k in 0..2 {
            if has_gap[k] {
                summary.missed[k].0 += 1;
                summary.missed[k].1 += missed[k];
            }
        }
        summary.capped += usize::from(capped);
        summary.weak += usize::from(weak);
        summary.failed_frames += usize::from(failed_frames);
    }
    summary
}

/// Seconds as the summary shows them, to a tenth and never zero.
fn seconds(s: f64) -> String {
    format!("{:.1}", s.max(0.1))
}

fn count(n: usize) -> FluentValue<'static> {
    FluentValue::from(n)
}

pub fn sentence(summary: &Summary, tr: &Translator) -> String {
    let mut counts = Vec::new();
    for (n, id) in [
        (summary.saved, "summary-saved"),
        (summary.review, "summary-review"),
        (summary.failed, "summary-failed"),
        (summary.cancelled, "summary-cancelled"),
    ] {
        if n > 0 {
            counts.push(tr.with(id, &[("count", count(n))]));
        }
    }
    if counts.is_empty() {
        counts.push(tr.get("summary-none"));
    }
    let mut details = Vec::new();
    for (k, id) in ["summary-missed-faces", "summary-missed-plates"]
        .iter()
        .enumerate()
    {
        let (videos, missed) = summary.missed[k];
        if videos > 0 {
            details.push(tr.with(
                id,
                &[
                    ("videos", count(videos)),
                    ("seconds", FluentValue::from(seconds(missed))),
                ],
            ));
        }
    }
    for (n, id) in [
        (summary.capped, "summary-capped"),
        (summary.weak, "summary-weak"),
        (summary.failed_frames, "summary-failed-frames"),
    ] {
        if n > 0 {
            details.push(tr.with(id, &[("count", count(n))]));
        }
    }
    let separator = tr.get("list-separator");
    let counts = counts.join(&separator);
    if details.is_empty() {
        counts
    } else {
        tr.with(
            "summary-with-details",
            &[
                ("counts", FluentValue::from(counts)),
                ("details", FluentValue::from(details.join(&separator))),
            ],
        )
    }
}

/// An unresolved problem of an item held for review, in words.
pub fn problem(e: &CoverageEvidence, video: Option<(u64, f64)>, tr: &Translator) -> String {
    match &e.problem {
        Problem::CappedCandidates { kept, found, .. } => tr.with(
            "problem-capped",
            &[
                ("kept", FluentValue::from(*kept)),
                ("found", FluentValue::from(*found)),
            ],
        ),
        Problem::FailedFrame { frame } => tr.with(
            "problem-failed-frame",
            &[("frame", FluentValue::from(*frame))],
        ),
        Problem::UnresolvedGap {
            detection,
            first,
            last,
            ..
        } => {
            let id = match detection {
                DetectionKind::Face => "problem-missed-face",
                DetectionKind::Plate => "problem-missed-plate",
            };
            tr.with(
                id,
                &[
                    ("first", FluentValue::from(*first)),
                    ("last", FluentValue::from(*last)),
                    (
                        "seconds",
                        FluentValue::from(seconds(
                            gap_seconds(*first, *last, video).unwrap_or(0.0),
                        )),
                    ),
                ],
            )
        }
        Problem::WeakCandidate { score, .. } => tr.with(
            "problem-weak",
            &[("score", FluentValue::from(format!("{score:.2}")))],
        ),
    }
}

#[cfg(test)]
mod tests {
    use cloakframe_core::detection::ModelId;
    use cloakframe_core::geometry::Rect;
    use cloakframe_core::item::PublishBasis;

    use super::*;
    use crate::i18n::Language;

    fn gap(kind: DetectionKind, first: u64, last: u64) -> CoverageEvidence {
        CoverageEvidence::unresolved(Problem::UnresolvedGap {
            detection: kind,
            tracks: vec![1],
            first,
            last,
        })
    }

    #[test]
    fn the_sentence_counts_the_outcomes_and_says_what_waits_for_review() {
        let saved = Item {
            state: ItemState::Published(PublishBasis::AutomaticUnreviewed),
            evidence: &[],
            video: None,
        };
        // 30 frames in a second: 36 frames are 1.2 seconds.
        let video_gaps = [
            gap(DetectionKind::Face, 30, 65),
            gap(DetectionKind::Plate, 0, 2),
        ];
        let weak = [CoverageEvidence::unresolved(Problem::WeakCandidate {
            model: ModelId::Yolo5FaceN,
            frame: None,
            bbox: Rect::new(0.0, 0.0, 4.0, 4.0).unwrap(),
            score: 0.31,
        })];
        let mut items: Vec<Item<'_>> = (0..12)
            .map(|_| Item {
                state: saved.state,
                evidence: &[],
                video: None,
            })
            .collect();
        items.push(Item {
            state: ItemState::NeedsReview,
            evidence: &video_gaps,
            video: Some((300, 10.0)),
        });
        items.push(Item {
            state: ItemState::NeedsReview,
            evidence: &weak,
            video: None,
        });
        let summary = summarize(items);
        assert_eq!(summary.missed[0].0, 1);
        assert!((summary.missed[0].1 - 1.2).abs() < 1e-9);
        let en = Translator::new(Language::En);
        assert_eq!(
            sentence(&summary, &en),
            "12 saved, 2 to review — faces missed for 1.2 s in one video, license plates missed \
             for 0.1 s in one video, an uncertain detection in one item"
        );
        let ko = Translator::new(Language::Ko);
        assert_eq!(
            sentence(&summary, &ko),
            "12개 저장, 2개 검토 필요 — 영상 1개에서 얼굴을 1.2초간 놓침, 영상 1개에서 번호판을 \
             0.1초간 놓침, 1개 항목에 확실하지 않은 감지"
        );
    }

    #[test]
    fn resolved_problems_and_other_outcomes_say_nothing_more() {
        let mut resolved = gap(DetectionKind::Face, 0, 9);
        resolved.resolution = cloakframe_core::coverage::Resolution::CoveredByManualMask;
        let items = [
            Item {
                state: ItemState::NeedsReview,
                evidence: std::slice::from_ref(&resolved),
                video: Some((10, 1.0)),
            },
            Item {
                state: ItemState::Failed,
                evidence: &[],
                video: None,
            },
            Item {
                state: ItemState::Cancelled,
                evidence: &[],
                video: None,
            },
        ];
        let en = Translator::new(Language::En);
        assert_eq!(
            sentence(&summarize(items), &en),
            "1 to review, 1 failed, 1 cancelled"
        );
        assert_eq!(sentence(&Summary::default(), &en), "Nothing was saved");
    }

    #[test]
    fn a_gap_reads_in_frames_and_seconds() {
        let en = Translator::new(Language::En);
        let face = gap(DetectionKind::Face, 30, 65);
        assert_eq!(
            problem(&face, Some((300, 10.0)), &en),
            "Face lost in frames 30–65 (1.2 s)"
        );
        assert_eq!(gap_seconds(0, 0, Some((0, 0.0))), None);
    }
}
