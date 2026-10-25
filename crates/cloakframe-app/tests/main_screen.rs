use std::path::{Path, PathBuf};
use std::sync::Arc;

use cloakframe_app::app::AppState;
use cloakframe_app::fonts;
use cloakframe_app::i18n::Language;
use cloakframe_app::run::Folders;
use cloakframe_app::settings::Settings;
use cloakframe_app::tools::Tools;
use cloakframe_core::item::{ItemState, PublishBasis};
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
    harness.get_by_label("One saved, 0 to review, 0 failed");
    harness.get_by_label("Saved");
    harness
        .get_by_role_and_label(Role::Button, "New run")
        .click();
    harness.run();
    assert!(harness.state().board.is_none());
}
