use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;

use cloakframe_app::app::{AppState, Command};
use cloakframe_app::fonts;
use cloakframe_app::i18n::Language;
use cloakframe_app::models::Models;
use cloakframe_app::settings::Settings;
use cloakframe_core::detection::ModelId;
use cloakframe_engine::model_files::download::{Body, Fetch, FetchError};
use cloakframe_engine::model_files::{MODEL_FILES, ModelStore, Status, model_file};
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

/// Serves each pinned model from the test assets, where the engine tests keep them.
struct FromAssets(PathBuf);

impl Fetch for FromAssets {
    fn get(&self, url: &str) -> Result<Body, FetchError> {
        let file = MODEL_FILES
            .iter()
            .find(|f| f.url == url)
            .ok_or_else(|| FetchError::NotAllowed(url.to_owned()))?;
        let opened =
            File::open(self.0.join(file.name)).map_err(|e| FetchError::Network(e.to_string()))?;
        let length = opened.metadata().ok().map(|m| m.len());
        Ok(Body {
            length,
            reader: Box::new(opened),
        })
    }
}

fn assets() -> PathBuf {
    std::env::var_os("CLOAKFRAME_TEST_ASSETS")
        .map(PathBuf::from)
        .expect("CLOAKFRAME_TEST_ASSETS is not set (see CONTRIBUTING.md)")
}

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn harness(folder: &Dir) -> Harness<'static, AppState> {
    let mut state = AppState::new(
        Settings {
            language: Some(Language::En),
            ..Settings::default()
        },
        None,
        Vec::new(),
    );
    state.models = Models::with_fetch(
        Some(folder.0.clone()),
        false,
        Arc::new(FromAssets(assets())),
    );
    Harness::builder()
        .with_size(egui::vec2(1100.0, 1400.0))
        .with_context_setup(|ctx| ctx.set_fonts(fonts::definitions(Language::En)))
        .build_ui_state(|ui, state: &mut AppState| state.ui(ui), state)
}

/// Steps while the threads work, then lets the window settle.
fn settle(harness: &mut Harness<'_, AppState>) {
    harness.step();
    let ctx = harness.ctx.clone();
    harness.state_mut().models.wait(&ctx);
    harness.run();
}

fn status(harness: &Harness<'_, AppState>, model: ModelId) -> Status {
    harness.state().models.statuses[&model].clone().unwrap()
}

#[test]
fn models_are_downloaded_only_after_their_terms_are_accepted() {
    let folder =
        Dir(std::env::temp_dir().join(format!("cloakframe-models-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&folder.0);
    let mut harness = harness(&folder);
    harness.state_mut().command(Command::Models);
    settle(&mut harness);
    for file in &MODEL_FILES {
        assert_eq!(status(&harness, file.model), Status::Missing);
    }
    assert_eq!(harness.get_all_by_label("Not downloaded").count(), 3);
    harness.get_by_label_contains("non-commercial purposes only");

    // YOLO5Face-n's terms limit its use: its button does nothing until they are accepted.
    harness.get_all_by_label("Download").next().unwrap().click();
    settle(&mut harness);
    assert_eq!(status(&harness, ModelId::Yolo5FaceN), Status::Missing);
    harness.get_by_label("I accept these terms").click();
    harness.run();
    harness.get_all_by_label("Download").next().unwrap().click();
    settle(&mut harness);
    assert_eq!(status(&harness, ModelId::Yolo5FaceN), Status::Present);
    assert!(
        ModelStore::new(&folder.0)
            .path(ModelId::Yolo5FaceN)
            .is_file()
    );
    harness.get_by_label("Downloaded");

    // The others have no such terms.
    harness.get_all_by_label("Download").next().unwrap().click();
    settle(&mut harness);
    assert_eq!(status(&harness, ModelId::YuNet), Status::Present);
}

#[test]
fn a_different_file_in_a_models_place_is_named_as_such() {
    let folder = Dir(std::env::temp_dir().join(format!(
        "cloakframe-models-different-{}",
        std::process::id()
    )));
    let _ = std::fs::remove_dir_all(&folder.0);
    std::fs::create_dir_all(&folder.0).unwrap();
    std::fs::write(
        folder.0.join(model_file(ModelId::YuNet).name),
        b"not the model",
    )
    .unwrap();
    let mut harness = harness(&folder);
    harness.state_mut().command(Command::Models);
    settle(&mut harness);
    assert_eq!(status(&harness, ModelId::YuNet), Status::Different);
    harness.get_by_label_contains("is not this model");
}
