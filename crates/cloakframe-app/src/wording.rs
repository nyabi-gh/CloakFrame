//! The engine's notes, failures and refusals in the window's language. A failure's own English,
//! as the CLI prints it, stays beside the translation as its detail.

use std::path::PathBuf;

use cloakframe_core::item::ItemId;
use cloakframe_engine::batch::LeftOut;
use cloakframe_engine::review::Rejection;
use cloakframe_engine::run::{Failure, FailureKind, Note};
use cloakframe_engine::startup::StartNote;
use fluent_bundle::FluentValue;

use crate::i18n::Translator;

/// Something a run or a review tells the user besides its items' states.
#[derive(Debug, Clone, PartialEq)]
pub enum RunNote {
    /// What an earlier run left in the app's data folder that the sweep could not remove.
    NotSwept {
        path: PathBuf,
        error: String,
    },
    ModelCopied {
        name: &'static str,
        from: PathBuf,
    },
    LegacyCoremlCacheLeft(String),
    UnusedCoremlCachesLeft(String),
    NoEstimate(String),
    SessionNotSaved(String),
    RecordsLeft {
        path: PathBuf,
        error: String,
    },
    LeftOut {
        item: ItemId,
        why: LeftOut,
    },
}

impl RunNote {
    pub fn of_start(note: &StartNote) -> Self {
        match note {
            StartNote::Copied { model, from } => Self::ModelCopied {
                name: cloakframe_engine::model_files::model_file(*model).name,
                from: from.parent().unwrap_or(from).to_path_buf(),
            },
            StartNote::LegacyCoremlCache(e) => Self::LegacyCoremlCacheLeft(e.to_string()),
            StartNote::UnusedCoremlCaches(e) => Self::UnusedCoremlCachesLeft(e.to_string()),
        }
    }
}

fn arg<'a>(value: impl Into<FluentValue<'a>>) -> FluentValue<'a> {
    value.into()
}

/// `name` is the item's file name, for the notes about one item.
pub fn run_note(note: &RunNote, name: impl Fn(ItemId) -> String, tr: &Translator) -> String {
    let path = |p: &PathBuf| p.display().to_string();
    match note {
        RunNote::NotSwept { path: p, error } => tr.with(
            "run-note-not-swept",
            &[("path", arg(path(p))), ("error", arg(error.as_str()))],
        ),
        RunNote::ModelCopied { name, from } => tr.with(
            "run-note-model-copied",
            &[("name", arg(*name)), ("from", arg(path(from)))],
        ),
        RunNote::LegacyCoremlCacheLeft(e) => {
            tr.with("run-note-legacy-coreml", &[("error", arg(e.as_str()))])
        }
        RunNote::UnusedCoremlCachesLeft(e) => {
            tr.with("run-note-unused-coreml", &[("error", arg(e.as_str()))])
        }
        RunNote::NoEstimate(e) => tr.with("run-note-no-estimate", &[("error", arg(e.as_str()))]),
        RunNote::SessionNotSaved(e) => {
            tr.with("run-note-session-not-saved", &[("error", arg(e.as_str()))])
        }
        RunNote::RecordsLeft { path: p, error } => tr.with(
            "run-note-records-left",
            &[("path", arg(path(p))), ("error", arg(error.as_str()))],
        ),
        RunNote::LeftOut { item, why } => {
            let id = match why {
                LeftOut::NotAnalyzed => "run-note-left-out-not-analyzed",
                LeftOut::SourceChanged => "run-note-left-out-changed",
            };
            tr.with(id, &[("name", arg(name(*item)))])
        }
    }
}

/// What kind of failure an item's was, in a sentence.
pub fn failure(failure: &Failure, tr: &Translator) -> String {
    tr.get(match failure.kind {
        FailureKind::Unsupported => "failure-unsupported",
        FailureKind::TooLarge => "failure-too-large",
        FailureKind::Unreadable => "failure-unreadable",
        FailureKind::SourceChanged => "failure-source-changed",
        FailureKind::NameTaken => "failure-name-taken",
        FailureKind::Output => "failure-output",
        FailureKind::Encode => "failure-encode",
        FailureKind::Detection => "failure-detection",
        FailureKind::Tool => "failure-tool",
        FailureKind::Other => "failure-other",
    })
}

/// The sentence and the engine's English after it, for the log.
pub fn failure_with_detail(f: &Failure, tr: &Translator) -> String {
    format!("{} ({})", failure(f, tr), f.reason)
}

pub fn note(note: &Note, tr: &Translator) -> String {
    match note {
        Note::ColorConverted => tr.get("note-color-converted"),
        Note::ColorProfileIgnored => tr.get("note-color-profile-ignored"),
        Note::NothingDetected => tr.get("note-nothing-detected"),
        Note::Tracked { faces, plates } => {
            let id = match (faces, plates) {
                (0, 0) => "note-tracked-nothing",
                (_, 0) => "note-tracked-faces",
                (0, _) => "note-tracked-plates",
                _ => "note-tracked-both",
            };
            tr.with(id, &[("faces", arg(*faces)), ("plates", arg(*plates))])
        }
        Note::AnalyzedAgainOnCpu => tr.get("note-analyzed-again-on-cpu"),
        Note::EncodedAgainInSoftware { encoder, reason } => tr.with(
            "note-encoded-again-in-software",
            &[("encoder", arg(*encoder)), ("reason", arg(reason.as_str()))],
        ),
        Note::Padded { width, height } => tr.with(
            "note-padded",
            &[("width", arg(*width)), ("height", arg(*height))],
        ),
        Note::FolderNotFlushed => tr.get("note-folder-not-flushed"),
        Note::TempFileLeft => tr.get("note-temp-file-left"),
        Note::RecordsLeft => tr.get("note-records-left"),
    }
}

/// Why the engine refused a decision. The refusals the app's own decisions should never meet
/// keep the engine's English as their detail.
pub fn refusal(rejection: &Rejection, tr: &Translator) -> String {
    match rejection {
        Rejection::SettingsChanged => tr.get("refused-settings"),
        Rejection::SourceChanged => tr.get("session-item-changed"),
        Rejection::Stale { .. } | Rejection::ForeignKey => tr.get("refused-stale"),
        Rejection::NameTaken { suggestion } => tr.with(
            "refused-name",
            &[("name", arg(suggestion.display().to_string()))],
        ),
        Rejection::InvalidName => tr.get("refused-invalid-name"),
        Rejection::UnresolvedProblems => tr.get("refused-unresolved"),
        Rejection::NotCovered { .. } => tr.get("refused-not-covered"),
        Rejection::UnknownItem
        | Rejection::DecidedTwice
        | Rejection::NoSuchEvidence(_)
        | Rejection::NothingToCover { .. }
        | Rejection::Edit(_) => tr.with(
            "refused-internal",
            &[("reason", arg(rejection.to_string()))],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    /// Every kind, note and refusal has its own sentence in each language, which the
    /// translations' id check then covers.
    #[test]
    fn every_engine_value_is_worded_in_every_language() {
        let kinds = [
            FailureKind::Unsupported,
            FailureKind::TooLarge,
            FailureKind::Unreadable,
            FailureKind::SourceChanged,
            FailureKind::NameTaken,
            FailureKind::Output,
            FailureKind::Encode,
            FailureKind::Detection,
            FailureKind::Tool,
            FailureKind::Other,
        ];
        let notes = [
            Note::ColorConverted,
            Note::ColorProfileIgnored,
            Note::NothingDetected,
            Note::Tracked {
                faces: 2,
                plates: 0,
            },
            Note::AnalyzedAgainOnCpu,
            Note::EncodedAgainInSoftware {
                encoder: "h264_videotoolbox",
                reason: "cannot encode the video".to_owned(),
            },
            Note::Padded {
                width: 642,
                height: 362,
            },
            Note::FolderNotFlushed,
            Note::TempFileLeft,
            Note::RecordsLeft,
        ];
        for language in Language::ALL {
            let tr = Translator::new(language);
            let mut sentences: Vec<String> = kinds
                .iter()
                .map(|&kind| {
                    failure(
                        &Failure {
                            kind,
                            reason: String::new(),
                        },
                        &tr,
                    )
                })
                .collect();
            sentences.dedup();
            assert_eq!(sentences.len(), kinds.len(), "{language:?}");
            let worded: Vec<String> = notes.iter().map(|n| note(n, &tr)).collect();
            assert!(worded.iter().all(|w| !w.is_empty()), "{language:?}");
            assert!(worded[5].contains("h264_videotoolbox") && worded[5].contains("cannot encode"));
            assert!(worded[6].contains("642"));
        }
        let en = Translator::new(Language::En);
        assert_eq!(
            note(
                &Note::Tracked {
                    faces: 1,
                    plates: 2
                },
                &en
            ),
            "Tracked one face and 2 license plates"
        );
        assert_eq!(
            note(
                &Note::Tracked {
                    faces: 0,
                    plates: 1
                },
                &en
            ),
            "Tracked one license plate"
        );
    }
}
