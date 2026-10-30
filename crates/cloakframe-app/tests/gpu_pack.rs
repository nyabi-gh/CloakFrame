use std::path::PathBuf;
use std::sync::Arc;

use cloakframe_app::fonts;
use cloakframe_app::gpu_pack::GpuPackRow;
use cloakframe_app::i18n::{Language, Translator};
use cloakframe_engine::gpu_pack::{ARCHIVES, GpuPack, Status};
use cloakframe_engine::model_files::download::{Body, Fetch, FetchError};
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

mod support;
use support::RunTraced;

/// Answers every request with a 404.
struct Nothing;

impl Fetch for Nothing {
    fn get(&self, _: &str) -> Result<Body, FetchError> {
        Err(FetchError::Status(404))
    }
}

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("cloakframe-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        Self(path)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct State {
    row: GpuPackRow,
    tr: Translator,
    restart: bool,
}

fn harness(dir: &Dir) -> Harness<'static, State> {
    let mut row = GpuPackRow::with_fetch(Some(GpuPack::new(&dir.0)), Arc::new(Nothing));
    row.check();
    let state = State {
        row,
        tr: Translator::new(Language::En),
        restart: false,
    };
    Harness::builder()
        .with_size(egui::vec2(700.0, 500.0))
        .with_context_setup(|ctx| ctx.set_fonts(fonts::definitions(Language::En)))
        .build_ui_state(
            |ui, state: &mut State| {
                state.row.take();
                state.row.rows(ui, &state.tr, state.restart);
                support::record(ui.ctx());
            },
            state,
        )
}

/// Files of the pinned sizes, without their bytes.
fn install_sparse(dir: &Dir) {
    let pack = GpuPack::new(&dir.0);
    std::fs::create_dir_all(pack.dir()).unwrap();
    for member in ARCHIVES.iter().flat_map(|a| a.members) {
        std::fs::File::create(pack.dir().join(member.name))
            .unwrap()
            .set_len(member.size)
            .unwrap();
    }
}

#[test]
fn the_pack_downloads_only_after_nvidias_terms_are_accepted() {
    let dir = Dir::new("gpu-pack-terms");
    let mut harness = harness(&dir);
    harness.run_traced();
    harness.get_by_label("NVIDIA CUDA Toolkit EULA");
    harness.get_by_label("NVIDIA cuDNN Software License Agreement");
    harness.get_by_label_contains("1217 MB download, 1870 MB installed");

    harness.get_by_label("Download").click();
    harness.run_traced();
    assert!(
        !harness.state().row.is_busy(),
        "the button is off until the terms are accepted"
    );

    harness.get_by_label("I accept these terms").click();
    harness.run_traced();
    harness.get_by_label("Download").click();
    harness.step();
    harness.state_mut().row.wait();
    harness.run_traced();
    harness.get_by_label_contains("The download failed: the server answered HTTP 404");
    assert_eq!(harness.state().row.status, Some(Status::Missing));
}

#[test]
fn an_installed_pack_can_be_removed_and_says_when_a_restart_is_due() {
    let dir = Dir::new("gpu-pack-installed");
    install_sparse(&dir);
    let mut harness = harness(&dir);
    harness.run_traced();
    assert_eq!(harness.state().row.status, Some(Status::Ready));
    harness.get_by_label("Downloaded");
    assert!(harness.query_by_label("I accept these terms").is_none());
    assert!(
        harness
            .query_by_label_contains("Restart CloakFrame")
            .is_none()
    );

    harness.state_mut().restart = true;
    harness.run_traced();
    harness.get_by_label("Restart CloakFrame to use the GPU pack.");

    harness.get_by_label("Remove").click();
    harness.run_traced();
    assert_eq!(harness.state().row.status, Some(Status::Missing));
    assert!(!GpuPack::new(&dir.0).dir().exists());
}

#[test]
fn a_damaged_pack_says_so_and_offers_the_download_again() {
    let dir = Dir::new("gpu-pack-damaged");
    install_sparse(&dir);
    let pack = GpuPack::new(&dir.0);
    std::fs::remove_file(pack.dir().join("libcudnn.so.9")).unwrap();
    let mut harness = harness(&dir);
    harness.run_traced();
    assert!(matches!(
        harness.state().row.status,
        Some(Status::Damaged(_))
    ));
    harness.get_by_label_contains("The GPU pack is incomplete and is not used");
    harness.get_by_label("Download");
}
