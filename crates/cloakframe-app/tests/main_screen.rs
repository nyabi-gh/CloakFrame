use std::path::{Path, PathBuf};
use std::sync::Arc;

use cloakframe_app::app::{AppState, Quit};
use cloakframe_app::fonts;
use cloakframe_app::i18n::Language;
use cloakframe_app::run::Folders;
use cloakframe_app::settings::Settings;
use cloakframe_app::tools::Tools;
use cloakframe_core::coverage::{CoverageEvidence, Problem};
use cloakframe_core::detection::{DetectionKind, ModelId};
use cloakframe_core::item::{ItemId, ItemState, PublishBasis};
use cloakframe_core::review::{Digest, ReviewKey, ReviewRevision};
use cloakframe_engine::identity::SourceStamp;
use cloakframe_engine::session::{FORMAT_VERSION, Session, SessionId, SessionItem, StoredMedia};
use eframe::egui;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("cloakframe-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn file(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, b"x").unwrap();
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Err("not read".to_owned())
    }
}

/// The window in `language`, its fonts in before the first frame as the app does. egui_kittest
/// fails on any character the fonts cannot draw.
fn harness(language: Language, settings: Settings) -> Harness<'static, AppState> {
    let state = AppState::new(
        Settings {
            language: Some(language),
            ..settings
        },
        None,
        Vec::new(),
    );
    Harness::builder()
        .with_size(egui::vec2(1100.0, 760.0))
        .with_context_setup(move |ctx| ctx.set_fonts(fonts::definitions(language)))
        .build_ui_state(|ui, state: &mut AppState| state.ui(ui), state)
}

fn drop_files(harness: &mut Harness<'_, AppState>, paths: &[PathBuf]) {
    for path in paths {
        harness
            .input_mut()
            .dropped_files
            .push(Arc::new(Dropped(path.clone())));
    }
    // One frame takes the drop and starts the scan, whose spinner repaints until it ends.
    harness.step();
    harness.state_mut().wait_for_scan();
    harness.run();
}

#[test]
fn every_language_draws_the_main_screen() {
    for language in Language::ALL {
        let mut harness = harness(language, Settings::default());
        harness.run();
        let start = harness.state().tr().get("start");
        harness.get_by_role_and_label(Role::Button, &start);
    }
}

#[test]
fn dropped_files_are_scanned_and_counted() {
    let dir = Dir::new("drop");
    let files = [
        dir.file("髙橋.jpg"),
        dir.file("家族.png"),
        dir.file("clip.mp4"),
        dir.file("notes.txt"),
    ];
    let mut harness = harness(Language::Ja, Settings::default());
    harness.state_mut().tools = Tools {
        ffmpeg: Some(dir.file("ffmpeg")),
        ffprobe: Some(dir.file("ffprobe")),
        ..Tools::default()
    };
    drop_files(&mut harness, &files);
    let found = harness.state().found.as_ref().unwrap();
    assert_eq!(found.files.len(), 3);
    harness.get_by_label("写真 2 件、動画 1 件");
    harness.get_by_label("txt 形式のファイル 1 件は処理できません");
    harness.get_by_label_contains("髙橋.jpg");

    // Without FFmpeg the videos are left out, and the window says why.
    let mut harness = harness_without_tools(&files);
    let found = harness.state().found.as_ref().unwrap();
    assert_eq!(found.files.len(), 2);
    harness.get_by_label("mp4 形式のファイル 1 件は処理できません");
    let why = harness.state().tr().get("videos-need-ffmpeg");
    harness.get_by_label(&why);
    harness.run();
}

fn harness_without_tools(files: &[PathBuf]) -> Harness<'static, AppState> {
    let mut harness = harness(Language::Ja, Settings::default());
    drop_files(&mut harness, files);
    harness
}

#[test]
fn start_waits_for_models_files_and_a_folder() {
    let dir = Dir::new("start");
    let output = dir.0.join("out");
    let mut harness = harness(Language::En, Settings::default());
    harness.run();
    assert_eq!(harness.state().start_blocker(), Some("start-needs-files"));
    drop_files(&mut harness, &[dir.file("a.jpg")]);
    assert_eq!(harness.state().start_blocker(), Some("start-needs-output"));
    harness.state_mut().settings.output = Some(output);
    harness.run();
    assert_eq!(harness.state().start_blocker(), Some("start-needs-runtime"));
    harness.state_mut().tools.ort_library = Some(dir.file("libonnxruntime"));
    harness.run();
    assert_eq!(harness.state().start_blocker(), None);
    harness
        .get_by_role_and_label(Role::CheckBox, "Faces")
        .click();
    harness.run();
    assert!(!harness.state().settings.faces);
    assert_eq!(harness.state().start_blocker(), Some("models-need-one"));
    harness.get_by_label("Choose faces, license plates, or both.");
}

#[test]
fn the_language_menu_changes_the_text() {
    let mut harness = harness(Language::En, Settings::default());
    harness.run();
    harness.get_by_label("View").click();
    harness.run();
    harness.get_by_label("한국어").click();
    harness.run();
    assert_eq!(harness.state().settings.language, Some(Language::Ko));
    harness.get_by_role_and_label(Role::Button, "시작");
}

/// The notices and the font license are drawn in full, in the bundled fonts only.
#[test]
fn the_about_window_draws_in_every_language() {
    for language in Language::ALL {
        let mut harness = harness(language, Settings::default());
        harness.state_mut().about_open = true;
        harness.run();
        let title = harness.state().tr().get("about-title");
        harness.get_by_label(&title);
    }
}

fn required_env(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{name} is not set (see CONTRIBUTING.md)"))
}

/// Start runs the engine on its own threads, and the window shows each item's state and the
/// summary. Needs the ONNX Runtime and the models the engine tests use (CONTRIBUTING.md).
#[test]
fn start_runs_the_engine_and_shows_the_result() {
    let dir = Dir::new("run");
    let input = dir.0.join("in");
    std::fs::create_dir_all(&input).unwrap();
    let assets = required_env("CLOAKFRAME_TEST_ASSETS");
    std::fs::copy(assets.join("lena.jpg"), input.join("lena.jpg")).unwrap();
    let output = dir.0.join("out");
    let settings = Settings {
        output: Some(output.clone()),
        review: false,
        ..Settings::default()
    };
    let mut harness = harness(Language::En, settings);
    harness.state_mut().tools = Tools {
        ort_library: Some(required_env("CLOAKFRAME_ORT_LIBRARY")),
        ..Tools::default()
    };
    harness.state_mut().folders = Folders {
        data: Some(dir.0.join("data")),
        models: Some(assets),
        app_models: false,
    };
    drop_files(&mut harness, &[input]);
    harness.get_by_role_and_label(Role::Button, "Start").click();
    // The spinner repaints while the run works, so the harness steps instead of settling.
    harness.step();
    harness.state_mut().wait_for_run();
    harness.run();
    let board = harness.state().board.as_ref().unwrap();
    assert!(board.failure.is_none(), "{:?}", board.failure);
    let finished = board.finished.as_ref().unwrap();
    assert_eq!(
        finished.report.items[0].state,
        ItemState::Published(PublishBasis::AutomaticUnreviewed),
        "{:?}",
        finished.report.items[0].failure
    );
    assert!(output.join("lena.jpg").is_file());
    harness.get_by_label("1 saved");
    harness.get_by_label("Saved");
    harness.get_by_role_and_label(Role::Button, "Show in Folder");
    harness.get_by_label("Details").click();
    harness.run();
    harness.get_by_label_contains("lena.jpg  Saved");
    harness
        .get_by_role_and_label(Role::Button, "New run")
        .click();
    harness.run();
    assert!(harness.state().board.is_none());
}

fn session_item(
    item: u64,
    input: PathBuf,
    media: StoredMedia,
    evidence: Vec<CoverageEvidence>,
) -> SessionItem {
    let key = ReviewKey {
        input: Digest([1; 32]),
        models: Digest([2; 32]),
        settings: Digest([3; 32]),
        timeline: Digest([4; 32]),
    };
    SessionItem {
        item: ItemId(item),
        destination: PathBuf::from(input.file_name().unwrap()),
        conflict: None,
        suggestion: None,
        source: SourceStamp::of(&input).unwrap(),
        input,
        revision: ReviewRevision::first(key),
        media,
        evidence,
    }
}

/// A session a run left is listed when the app starts again, opens to its items and their
/// problems, and is discarded only once the user confirms.
#[test]
fn reviews_kept_by_earlier_runs_are_listed_opened_and_discarded() {
    let dir = Dir::new("sessions");
    let output = dir.0.join("out");
    let folders = Folders {
        data: Some(dir.0.join("data")),
        models: None,
        app_models: false,
    };
    let photo = dir.file("photo.jpg");
    let clip = dir.file("clip.mp4");
    let gone = dir.file("gone.jpg");
    let capped = CoverageEvidence::unresolved(Problem::CappedCandidates {
        model: ModelId::YuNet,
        frame: None,
        kept: 100,
        found: 259,
    });
    let gap = CoverageEvidence::unresolved(Problem::UnresolvedGap {
        detection: DetectionKind::Face,
        tracks: vec![1, 2],
        first: 30,
        last: 65,
    });
    let photo_media = || StoredMedia::Photo {
        detections: Vec::new(),
        edits: Default::default(),
    };
    let session = Session {
        version: FORMAT_VERSION,
        options: None,
        output: output.clone(),
        items: vec![
            session_item(0, photo, photo_media(), vec![capped]),
            session_item(
                1,
                clip,
                StoredMedia::Video {
                    frames: 300,
                    width: 64,
                    height: 48,
                    seconds: 10.0,
                    tracks: Vec::new(),
                    edits: Default::default(),
                },
                vec![gap],
            ),
            session_item(2, gone.clone(), photo_media(), Vec::new()),
        ],
    };
    let store = folders.sessions().unwrap();
    let id = SessionId::generate();
    store.save(&id, &session).unwrap();
    std::fs::remove_file(&gone).unwrap();

    let mut harness = harness(Language::En, Settings::default());
    harness.state_mut().set_folders(folders);
    harness.step();
    harness.state_mut().wait_for_sessions();
    harness.run();
    harness.get_by_label("Waiting for review");
    harness.get_by_label("Today");
    harness.get_by_label(&format!("3 items for {}", output.display()));
    harness.get_by_label("one original changed");

    harness.get_by_role_and_label(Role::Button, "Open").click();
    harness.run();
    harness.get_by_label(
        "3 to review — faces missed for 1.2 s in one video, the detection limit reached in one item",
    );
    harness.get_by_label("Face lost in frames 30–65 (1.2 s)");
    harness.get_by_label("Detection limit reached: 100 of 259 kept");
    harness.get_by_label("The original changed or is gone; run it again");
    harness.get_by_role_and_label(Role::Button, "Back").click();
    harness.run();
    assert!(harness.state().opened.is_none());

    harness
        .get_by_role_and_label(Role::Button, "Discard…")
        .click();
    harness.run();
    harness.get_by_label("Discard this review?");
    harness.get_by_role_and_label(Role::Button, "Keep").click();
    harness.run();
    assert!(store.load(&id).is_ok(), "kept until the user confirms");
    harness
        .get_by_role_and_label(Role::Button, "Discard…")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Discard")
        .click();
    harness.run();
    assert!(store.list().unwrap().is_empty());
    assert!(harness.state().sessions.list.is_empty());
    assert!(harness.query_by_label("Waiting for review").is_none());
}

fn close(harness: &mut Harness<'_, AppState>) {
    harness
        .input_mut()
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .events
        .push(egui::ViewportEvent::Close);
    harness.step();
}

fn commands(harness: &Harness<'_, AppState>) -> Vec<egui::ViewportCommand> {
    harness
        .output()
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map(|v| v.commands.clone())
        .unwrap_or_default()
}

/// Closing the window while a run works asks first; once the user agrees, the run is
/// cancelled and the window closes after it has stopped and kept its review. Needs the ONNX
/// Runtime and the models (CONTRIBUTING.md).
#[test]
fn closing_during_a_run_asks_and_waits_for_the_run_to_stop() {
    let dir = Dir::new("quit");
    let input = dir.0.join("in");
    std::fs::create_dir_all(&input).unwrap();
    let assets = required_env("CLOAKFRAME_TEST_ASSETS");
    for i in 0..8 {
        std::fs::copy(assets.join("lena.jpg"), input.join(format!("lena-{i}.jpg"))).unwrap();
    }
    let settings = Settings {
        output: Some(dir.0.join("out")),
        ..Settings::default()
    };
    let mut harness = harness(Language::En, settings);
    harness.state_mut().tools = Tools {
        ort_library: Some(required_env("CLOAKFRAME_ORT_LIBRARY")),
        ..Tools::default()
    };
    harness.state_mut().set_folders(Folders {
        data: Some(dir.0.join("data")),
        models: Some(assets),
        app_models: false,
    });
    drop_files(&mut harness, &[input]);
    harness.get_by_role_and_label(Role::Button, "Start").click();
    harness.step();

    close(&mut harness);
    assert!(commands(&harness).contains(&egui::ViewportCommand::CancelClose));
    assert_eq!(harness.state().quit, Quit::Asking);
    harness.step();
    harness.get_by_label("A run is in progress");
    harness
        .get_by_role_and_label(Role::Button, "Keep Working")
        .click();
    harness.step();
    assert_eq!(harness.state().quit, Quit::No);

    close(&mut harness);
    harness.step();
    harness
        .get_by_role_and_label(Role::Button, "Cancel and Quit")
        .click();
    harness.step();
    assert_eq!(harness.state().quit, Quit::AfterRun);
    harness.state_mut().wait_for_run();
    harness.step();
    assert!(commands(&harness).contains(&egui::ViewportCommand::Close));
    assert_eq!(harness.state().quit, Quit::No);
    let finished = harness
        .state()
        .board
        .as_ref()
        .unwrap()
        .finished
        .as_ref()
        .unwrap();
    assert!(
        finished
            .report
            .items
            .iter()
            .all(|o| matches!(o.state, ItemState::NeedsReview | ItemState::Cancelled)),
        "{:?}",
        finished
            .report
            .items
            .iter()
            .map(|o| o.state)
            .collect::<Vec<_>>()
    );
}

/// Every message of every language, the new screens' included, in the fonts the window uses for
/// it: egui_kittest fails on a character they cannot draw.
#[test]
fn every_message_of_every_language_can_be_drawn() {
    for (language, text) in [
        (Language::En, include_str!("../assets/i18n/en.ftl")),
        (Language::Ko, include_str!("../assets/i18n/ko.ftl")),
        (Language::Ja, include_str!("../assets/i18n/ja.ftl")),
    ] {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1100.0, 760.0))
            .with_context_setup(move |ctx| ctx.set_fonts(fonts::definitions(language)))
            .build_ui(move |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for line in text.lines() {
                        ui.label(line);
                    }
                });
            });
        harness.run();
    }
}
