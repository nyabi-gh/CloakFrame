use std::path::{Path, PathBuf};
use std::sync::Arc;

use cloakframe_app::app::{AppState, Quit};
use cloakframe_app::draft::Pick;
use cloakframe_app::fonts;
use cloakframe_app::i18n::Language;
use cloakframe_app::run::{Folders, RunOptions, processor};
use cloakframe_app::settings::{Method, Settings};
use cloakframe_app::tools::Tools;
use cloakframe_core::coverage::{CoverageEvidence, Problem};
use cloakframe_core::detection::{DetectionKind, ModelId};
use cloakframe_core::geometry::Rect;
use cloakframe_core::item::{ItemId, ItemState, PublishBasis};
use cloakframe_core::review::{Digest, ReviewKey, ReviewRevision};
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::Rgb8Image;
use cloakframe_engine::identity::{SourceStamp, content_digest};
use cloakframe_engine::media::decode::FrameReader;
use cloakframe_engine::media::source;
use cloakframe_engine::photo::budget::MemoryBudget;
use cloakframe_engine::photo::decode::{decode, read_source};
use cloakframe_engine::run::ItemProcessor;
use cloakframe_engine::session::{FORMAT_VERSION, Session, SessionId, SessionItem, StoredMedia};
use cloakframe_engine::video::process::item_records_dir;
use cloakframe_engine::video::record::{Order, RecordWriter, TrackRegion};
use cloakframe_engine::video::tracking::TrackSummary;
use eframe::egui;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

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

/// A session a run left is listed when the app starts again and opens to its queue: items with
/// unresolved problems first, each with what it waits for; the editor lists an item's problems.
/// It is discarded only once the user confirms.
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
            session_item(0, gone.clone(), photo_media(), Vec::new()),
            session_item(1, photo, photo_media(), vec![capped]),
            session_item(
                2,
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
    // The queue's thumbnails and the editor's photo are made on threads of their own; the
    // harness waits for them before it settles.
    harness.step();
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .review
        .as_mut()
        .unwrap()
        .wait_for_thumbnails(&ctx);
    harness.run();
    harness.get_by_label(
        "3 to review — faces missed for 1.2 s in one video, the detection limit reached in one item",
    );
    let order: Vec<u64> = harness
        .state()
        .review
        .as_ref()
        .unwrap()
        .review
        .order
        .iter()
        .map(|i| i.item.0)
        .collect();
    assert_eq!(order, [1, 2, 0], "unresolved problems first");
    assert!(store.lock(&id).is_err(), "held while it is open");

    harness.get_by_label_contains("photo.jpg").click();
    harness.step();
    harness
        .state_mut()
        .review
        .as_mut()
        .unwrap()
        .wait_for_picture(&ctx);
    harness.run();
    harness.get_by_label("Detection limit reached: 100 of 259 kept");
    harness.get_by_label("Not resolved");
    assert!(
        harness
            .get_by_role_and_label(Role::Button, "Approve and Save")
            .accesskit_node()
            .is_disabled()
    );
    harness
        .get_by_role_and_label(Role::Button, "Nothing to Mask")
        .click();
    harness.run();
    harness.get_by_label("Nothing to mask");
    assert!(
        !harness
            .get_by_role_and_label(Role::Button, "Approve and Save")
            .accesskit_node()
            .is_disabled()
    );
    harness.get_by_role_and_label(Role::Button, "Next").click();
    harness.step();
    harness
        .state_mut()
        .review
        .as_mut()
        .unwrap()
        .wait_for_picture(&ctx);
    harness.run();
    harness.get_by_label("Face lost in frames 30–65 (1.2 s)");
    harness.get_by_label("The video cannot be shown: this run does not process videos");
    harness.get_by_role_and_label(Role::Button, "Next").click();
    harness.step();
    harness
        .state_mut()
        .review
        .as_mut()
        .unwrap()
        .wait_for_picture(&ctx);
    harness.run();
    harness.get_by_label("The original changed or is gone; run it again");
    harness
        .get_by_role_and_label(Role::Button, "All Items")
        .click();
    harness.run();
    harness.get_by_label("gone.jpg, Original changed");
    harness.get_by_label("photo.jpg, To review");
    harness.get_by_label("clip.mp4, One problem");
    harness.get_by_role_and_label(Role::Button, "Back").click();
    harness.run();
    assert!(harness.state().review.is_none());
    assert!(store.lock(&id).is_ok(), "let go once it is closed");

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

fn pixels(path: &Path) -> image::RgbImage {
    let budget = MemoryBudget::new(1 << 30);
    let source = read_source(path).unwrap();
    decode(&source, &budget, &CancelToken::new())
        .unwrap()
        .image
        .to_rgb8()
}

/// The mean absolute difference of two images' pixels in `area`.
fn difference(a: &image::RgbImage, b: &image::RgbImage, area: Rect) -> f64 {
    let (x0, y0) = (area.x() as u32, area.y() as u32);
    let (x1, y1) = (area.right() as u32, area.bottom() as u32);
    let mut sum = 0u64;
    let mut n = 0u64;
    for y in y0..y1 {
        for x in x0..x1 {
            let (p, q) = (a.get_pixel(x, y).0, b.get_pixel(x, y).0);
            sum += (0..3).map(|c| u64::from(p[c].abs_diff(q[c]))).sum::<u64>();
            n += 3;
        }
    }
    sum as f64 / n as f64
}

/// A run held for review opens in the editor from its result. Clicking a detection leaves it
/// unmasked, undo and redo walk the changes, a drag adds an area, Space previews the masking and
/// `?` lists the shortcuts; the saved photo is masked as decided. Exporting without
/// anonymization asks first. Needs the ONNX Runtime and the models (CONTRIBUTING.md).
#[test]
fn the_editor_decides_what_is_masked_and_saves_it() {
    let dir = Dir::new("editor");
    let input = dir.0.join("in");
    std::fs::create_dir_all(&input).unwrap();
    let assets = required_env("CLOAKFRAME_TEST_ASSETS");
    for name in ["a.jpg", "b.jpg"] {
        std::fs::copy(assets.join("lena.jpg"), input.join(name)).unwrap();
    }
    let output = dir.0.join("out");
    let settings = Settings {
        output: Some(output.clone()),
        method: Method::Fill,
        ..Settings::default()
    };
    let mut harness = harness(Language::En, settings);
    harness.state_mut().tools = Tools {
        ort_library: Some(required_env("CLOAKFRAME_ORT_LIBRARY")),
        ..Tools::default()
    };
    let folders = Folders {
        data: Some(dir.0.join("data")),
        models: Some(assets.clone()),
        app_models: false,
    };
    harness.state_mut().set_folders(folders.clone());
    drop_files(&mut harness, std::slice::from_ref(&input));
    harness.get_by_role_and_label(Role::Button, "Start").click();
    harness.step();
    harness.state_mut().wait_for_run();
    harness.run();
    let rows = harness.query_all_by_label("Review").count();
    assert_eq!(rows, 2, "one Review button for each item held");
    // The review masks as the run would have, whatever the main screen says by then.
    harness.state_mut().settings.method = Method::Blur;
    harness
        .get_by_role_and_label(Role::Button, "Review Now")
        .click();
    harness.step();
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .review
        .as_mut()
        .unwrap()
        .wait_for_thumbnails(&ctx);
    harness.run();
    harness.get_by_label_contains("a.jpg").click();
    harness.step();
    harness
        .state_mut()
        .review
        .as_mut()
        .unwrap()
        .wait_for_picture(&ctx);
    harness.run();
    harness
        .state_mut()
        .review
        .as_mut()
        .unwrap()
        .wait_for_picture(&ctx);
    harness.run();

    let (face, view) = {
        let screen = harness.state().review.as_ref().unwrap();
        let item = &screen.review.order[0];
        let StoredMedia::Photo { detections, .. } = &item.media else {
            panic!("a photo")
        };
        assert_eq!(detections.len(), 1, "Lena's face");
        (
            detections[0].bbox(),
            screen.editor.as_ref().unwrap().view.unwrap(),
        )
    };
    let canvas = harness.get_by_label("Photo editor").rect();
    let origin = canvas.center() - view.center.to_vec2() * view.scale;
    let on_screen = |x: f32, y: f32| origin + egui::vec2(x, y) * view.scale;
    let item = ItemId(0);
    let draft =
        |h: &Harness<'_, AppState>| h.state().review.as_ref().unwrap().review.drafts[&item].clone();

    let center = face.center();
    harness.hover_at(on_screen(center.x, center.y));
    harness.step();
    harness.drag_at(on_screen(center.x, center.y));
    harness.step();
    harness.drop_at(on_screen(center.x, center.y));
    harness.step();
    assert!(
        draft(&harness).is_excluded(0),
        "a click leaves the face unmasked"
    );
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.step();
    assert!(!draft(&harness).is_excluded(0), "undone");
    harness.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::Z,
    );
    harness.step();
    assert!(draft(&harness).is_excluded(0), "redone");

    let added = Rect::new(20.0, 20.0, 60.0, 50.0).unwrap();
    harness.hover_at(on_screen(added.x(), added.y()));
    harness.step();
    harness.drag_at(on_screen(added.x(), added.y()));
    harness.step();
    for t in [0.3, 0.6, 1.0] {
        harness.hover_at(on_screen(
            added.x() + added.width() * t,
            added.y() + added.height() * t,
        ));
        harness.step();
    }
    harness.drop_at(on_screen(added.right(), added.bottom()));
    harness.step();
    let areas = draft(&harness).doc.edits.added;
    assert_eq!(areas.len(), 1, "a drag adds an area");
    assert!(
        (areas[0].x() - added.x()).abs() < 2.0 && (areas[0].width() - added.width()).abs() < 2.0,
        "{areas:?}"
    );
    harness.key_press(egui::Key::ArrowRight);
    harness.step();
    let moved = draft(&harness).doc.edits.added[0];
    assert_eq!(
        moved.x(),
        areas[0].x() + 1.0,
        "the arrow keys move the selected area"
    );

    harness.key_down(egui::Key::Space);
    harness.step();
    assert!(
        harness
            .state()
            .review
            .as_ref()
            .unwrap()
            .editor
            .as_ref()
            .unwrap()
            .previewing
    );
    harness
        .state_mut()
        .review
        .as_mut()
        .unwrap()
        .wait_for_picture(&ctx);
    harness.key_up(egui::Key::Space);
    harness.step();
    assert!(
        !harness
            .state()
            .review
            .as_ref()
            .unwrap()
            .editor
            .as_ref()
            .unwrap()
            .previewing
    );

    harness.event(egui::Event::Text("?".to_owned()));
    harness.step();
    harness.run();
    harness.get_by_label("Shortcuts");
    harness.get_by_label("Preview the masking");
    harness.get_by_role_and_label(Role::Button, "Close").click();
    harness.run();
    assert!(harness.query_by_label("Preview the masking").is_none());
    harness.key_press(egui::Key::Escape);
    harness.step();
    harness.key_press(egui::Key::Tab);
    harness.step();
    assert_eq!(
        draft(&harness).selected,
        Some(Pick::Detection(0)),
        "Tab selects the face"
    );
    harness.key_press(egui::Key::Enter);
    harness.step();
    assert!(!draft(&harness).is_excluded(0), "Enter masks it again");
    harness.key_press(egui::Key::Enter);
    harness.step();
    assert!(draft(&harness).is_excluded(0));

    harness
        .get_by_role_and_label(Role::Button, "Approve and Save")
        .click();
    // The next photo opens while this one renders.
    harness.step();
    harness
        .state_mut()
        .review
        .as_mut()
        .unwrap()
        .wait_for_picture(&ctx);
    harness.run();
    let screen = harness.state().review.as_ref().unwrap();
    assert_eq!(
        screen.editor.as_ref().map(|e| e.item),
        Some(ItemId(1)),
        "on to the next item"
    );

    harness.get_by_role_and_label(Role::Button, "More").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Export Without Anonymization…")
        .click();
    harness.run();
    harness.get_by_label("Export without anonymization?");
    harness
        .get_by_role_and_label(Role::Button, "Cancel")
        .click();
    harness.run();
    assert!(harness.state().review.as_ref().unwrap().confirm.is_none());
    assert!(
        harness
            .state()
            .review
            .as_ref()
            .unwrap()
            .review
            .is_open(ItemId(1)),
        "not decided"
    );
    harness.get_by_role_and_label(Role::Button, "More").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Export Without Anonymization…")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Export Without Anonymization")
        .click();
    harness.step();
    harness.state_mut().wait_for_review();
    harness.run();
    harness.get_by_label("Every item of this review is saved.");
    assert!(
        folders.sessions().unwrap().list().unwrap().is_empty(),
        "the finished session is removed"
    );

    let original = pixels(&input.join("a.jpg"));
    let masked = pixels(&output.join("a.jpg"));
    let plain = pixels(&output.join("b.jpg"));
    assert!(
        difference(&original, &masked, face) < 6.0,
        "the face was left unmasked"
    );
    assert!(
        difference(&original, &masked, moved) > 40.0,
        "the added area is filled"
    );
    assert!(difference(&original, &plain, face) < 6.0, "nothing masked");
    harness.get_by_role_and_label(Role::Button, "Back").click();
    harness.run();
    assert!(harness.state().review.is_none());
}

/// Where the test video's square is in frame `k`, in display pixels.
fn square_at(k: u64) -> Rect {
    Rect::new(40.0 + k as f32 * 2.0, 100.0, 60.0, 60.0).unwrap()
}

fn video_frames(path: &Path, ffmpeg: &Path, ffprobe: &Path) -> Vec<Rgb8Image> {
    let cancel = CancelToken::new();
    let source = source::open(ffprobe, path, &cancel).unwrap();
    let size = source.spec.display_size();
    let mut reader =
        FrameReader::start(ffmpeg, path, &source.spec, &source.frames, size, &cancel).unwrap();
    let mut frames = Vec::new();
    loop {
        let mut frame = Rgb8Image {
            width: 0,
            height: 0,
            data: Vec::new(),
        };
        if !reader.next_into(&mut frame).unwrap() {
            return frames;
        }
        frames.push(frame);
    }
}

/// The square's middle in frame `k`: white where it shows, under the fill's black where masked.
fn square_middle(frames: &[Rgb8Image], k: u64) -> u8 {
    let frame = &frames[k as usize];
    let c = square_at(k).center();
    let i = (c.y as usize * frame.width as usize + c.x as usize) * 3;
    frame.data[i]
}

/// A video's review: tracks are listed in the timeline and left unmasked, a gap is covered by a
/// track drawn over it from the tracks either side, frames are stepped through, previewed masked
/// and played, a track drawn and removed, and the decision renders as reviewed. The session is
/// what a run would keep, made with the square's tracks on either side of a gap.
#[test]
fn the_video_editor_edits_tracks_covers_a_gap_and_saves_it() {
    let dir = Dir::new("video-editor");
    let ffmpeg = required_env("CLOAKFRAME_FFMPEG");
    let ffprobe = required_env("CLOAKFRAME_FFPROBE");
    let input = dir.0.join("clip.mp4");
    let status = std::process::Command::new(&ffmpeg)
        .args(["-v", "error", "-f", "lavfi", "-i"])
        .arg(
            "color=c=black:s=640x360:r=30:d=4[bg];color=c=white:s=60x60:r=30[sq];\
             [bg][sq]overlay=x='40+t*60':y=100:shortest=1",
        )
        .args([
            "-c:v", "libx264", "-crf", "1", "-g", "30", "-pix_fmt", "yuv420p",
        ])
        .arg(&input)
        .status()
        .unwrap();
    assert!(status.success());

    let tools = Tools {
        ffmpeg: Some(ffmpeg.clone()),
        ffprobe: Some(ffprobe.clone()),
        ..Tools::default()
    };
    let folders = Folders {
        data: Some(dir.0.join("data")),
        models: None,
        app_models: false,
    };
    let output = dir.0.join("out");
    let options = RunOptions {
        method: Method::Fill,
        ..RunOptions::of(&Settings::default())
    };
    let store = folders.sessions().unwrap();
    let id = SessionId::generate();
    let records = item_records_dir(&store.records_dir(&id), ItemId(0));
    std::fs::create_dir_all(&records).unwrap();
    let mut regions =
        RecordWriter::<TrackRegion>::create(&records.join("regions"), Order::Ascending).unwrap();
    for k in 0..120 {
        let track = match k {
            0..40 => Some(1),
            80.. => Some(2),
            _ => None,
        };
        let entries: Vec<TrackRegion> = track
            .map(|track| TrackRegion {
                kind: DetectionKind::Face,
                track,
                bbox: square_at(k),
                score: 0.9,
                roll: None,
                interpolated: false,
            })
            .into_iter()
            .collect();
        regions.frame(k, &entries).unwrap();
    }
    regions.finish(&[]).unwrap();
    let source = source::open(&ffprobe, &input, &CancelToken::new()).unwrap();
    assert_eq!(source.frames.len(), 120);
    let fingerprint = processor(&options, &tools, store.records_dir(&id))
        .unwrap()
        .fingerprint(&input);
    let key = ReviewKey {
        input: content_digest(&std::fs::read(&input).unwrap()),
        models: fingerprint.models,
        settings: fingerprint.settings,
        timeline: source.timeline_digest(),
    };
    let summary = |id, first, last| TrackSummary {
        id,
        kind: DetectionKind::Face,
        first,
        last,
        detections: 40,
        strong: 40,
        low_confidence: false,
    };
    let session = Session {
        version: FORMAT_VERSION,
        options: Some(serde_json::to_value(options).unwrap()),
        output: output.clone(),
        items: vec![SessionItem {
            revision: ReviewRevision::first(key),
            ..session_item(
                0,
                input.clone(),
                StoredMedia::Video {
                    frames: 120,
                    width: 640,
                    height: 360,
                    seconds: 4.0,
                    tracks: vec![summary(1, 0, 39), summary(2, 80, 119)],
                    edits: Default::default(),
                },
                vec![CoverageEvidence::unresolved(Problem::UnresolvedGap {
                    detection: DetectionKind::Face,
                    tracks: vec![1, 2],
                    first: 40,
                    last: 79,
                })],
            )
        }],
    };
    store.save(&id, &session).unwrap();

    let mut harness = harness(Language::En, Settings::default());
    harness.state_mut().tools = tools;
    harness.state_mut().set_folders(folders.clone());
    harness.step();
    harness.state_mut().wait_for_sessions();
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Open").click();
    harness.step();
    let ctx = harness.ctx.clone();
    let wait = |h: &mut Harness<'_, AppState>| {
        h.state_mut()
            .review
            .as_mut()
            .unwrap()
            .wait_for_picture(&ctx);
    };
    harness
        .state_mut()
        .review
        .as_mut()
        .unwrap()
        .wait_for_thumbnails(&ctx);
    harness.run();
    harness.get_by_label_contains("clip.mp4").click();
    harness.step();
    wait(&mut harness);
    harness.step();
    wait(&mut harness);
    harness.run();
    harness.get_by_label("Frame 0 of 0–119 · 0:00.0");
    harness.get_by_label("2 tracks, 2 masked");
    harness.get_by_label("Timeline");

    let item = ItemId(0);
    let draft =
        |h: &Harness<'_, AppState>| h.state().review.as_ref().unwrap().review.drafts[&item].clone();
    let at = |h: &Harness<'_, AppState>| {
        let screen = h.state().review.as_ref().unwrap();
        screen.editor.as_ref().unwrap().video.as_ref().unwrap().at
    };
    let step = |h: &mut Harness<'_, AppState>| {
        h.step();
        wait(h);
        h.step();
    };

    harness.key_press(egui::Key::Tab);
    step(&mut harness);
    assert_eq!(draft(&harness).selected, Some(Pick::Track(1)));
    harness.key_press(egui::Key::Enter);
    step(&mut harness);
    assert!(
        draft(&harness).is_track_excluded(1),
        "Enter leaves it unmasked"
    );
    harness.run();
    harness.get_by_label("2 tracks, 1 masked");
    harness.get_by_role_and_label(Role::Button, "Mask This Track");

    harness.event(egui::Event::Text("]".to_owned()));
    step(&mut harness);
    assert_eq!(at(&harness), 40, "] goes to the gap");
    assert_eq!(draft(&harness).selected, Some(Pick::Problem(0)));
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Draw a Track Over It")
        .click();
    step(&mut harness);
    harness.run();
    harness.get_by_label("Covered by a mask");
    let drawn = draft(&harness).doc.video.added_tracks[0].clone();
    assert_eq!((drawn.first, drawn.last), (40, 79));
    assert_eq!(
        drawn.rect_at(40),
        Some(square_at(39)),
        "from track 1's last box"
    );
    assert_eq!(
        drawn.rect_at(79),
        Some(square_at(80)),
        "to track 2's first box"
    );

    harness.key_press(egui::Key::ArrowRight);
    step(&mut harness);
    assert_eq!(at(&harness), 41);
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowRight);
    step(&mut harness);
    assert_eq!(at(&harness), 71, "Shift moves a second");
    harness.key_press(egui::Key::End);
    step(&mut harness);
    assert_eq!(at(&harness), 119);
    harness.key_press(egui::Key::Home);
    step(&mut harness);
    assert_eq!(at(&harness), 0);
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowRight);
    step(&mut harness);
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowRight);
    step(&mut harness);
    assert_eq!(at(&harness), 60);

    harness.key_down(egui::Key::M);
    step(&mut harness);
    {
        let screen = harness.state().review.as_ref().unwrap();
        let clip = &screen.editor.as_ref().unwrap().video.as_ref().unwrap().clip;
        let shown = clip.shown.as_ref().unwrap();
        assert!(
            shown.masked && shown.frame == 60,
            "M shows the frame masked"
        );
    }
    harness.key_up(egui::Key::M);
    step(&mut harness);

    let view = harness
        .state()
        .review
        .as_ref()
        .unwrap()
        .editor
        .as_ref()
        .unwrap()
        .video
        .as_ref()
        .unwrap()
        .view()
        .unwrap();
    let canvas = harness.get_by_label("Video editor").rect();
    let origin = canvas.center() - view.center.to_vec2() * view.scale;
    let on_screen = |x: f32, y: f32| origin + egui::vec2(x, y) * view.scale;
    let area = Rect::new(400.0, 240.0, 80.0, 60.0).unwrap();
    harness.hover_at(on_screen(area.x(), area.y()));
    harness.step();
    harness.drag_at(on_screen(area.x(), area.y()));
    harness.step();
    for t in [0.3, 0.6, 1.0] {
        harness.hover_at(on_screen(
            area.x() + area.width() * t,
            area.y() + area.height() * t,
        ));
        harness.step();
    }
    harness.drop_at(on_screen(area.right(), area.bottom()));
    step(&mut harness);
    let tracks = draft(&harness).doc.video.added_tracks;
    assert_eq!(tracks.len(), 2, "a drag draws a track");
    assert_eq!((tracks[1].first, tracks[1].last), (60, 89), "for a second");
    assert_eq!(draft(&harness).selected, Some(Pick::Manual(1)));
    harness.key_press(egui::Key::Delete);
    step(&mut harness);
    assert_eq!(draft(&harness).doc.video.added_tracks.len(), 1);

    harness.key_press(egui::Key::Space);
    harness.step();
    // Playback shows the frame it starts from, then the next at its time.
    for _ in 0..2 {
        let played = harness
            .state_mut()
            .review
            .as_mut()
            .unwrap()
            .editor
            .as_mut()
            .unwrap()
            .video
            .as_mut()
            .unwrap()
            .clip
            .wait_for_playback(&ctx);
        assert!(played, "Space plays");
        harness.step();
    }
    harness.key_press(egui::Key::Space);
    step(&mut harness);
    harness.run();
    assert!(at(&harness) > 60, "playback moved on");
    harness.get_by_role_and_label(Role::Button, "Play");

    harness.event(egui::Event::Text("?".to_owned()));
    harness.step();
    harness.run();
    harness.get_by_label("Play or stop");
    harness.get_by_role_and_label(Role::Button, "Close").click();
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Approve and Save")
        .click();
    harness.step();
    harness.state_mut().wait_for_review();
    harness.run();
    harness.get_by_label("Every item of this review is saved.");

    let out = video_frames(&output.join("clip.mp4"), &ffmpeg, &ffprobe);
    assert_eq!(out.len(), 120);
    for k in [0, 20, 39] {
        assert!(
            square_middle(&out, k) > 200,
            "frame {k}: track 1 is not masked"
        );
    }
    for k in [40, 60, 79, 80, 100, 119] {
        assert!(square_middle(&out, k) < 30, "frame {k}: masked");
    }
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
