//! Pictures of every screen in both themes and every language, to set beside the design
//! (docs/M6_UX.md). Nothing is compared: font rendering differs between systems, so CI keeps no
//! golden images. Run it on a Mac with `--ignored` and look at what it writes to
//! `CLOAKFRAME_SNAPSHOT_DIR`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cloakframe_app::app::{AppState, Command};
use cloakframe_app::editor::ReviewScreen;
use cloakframe_app::fonts;
use cloakframe_app::i18n::Language;
use cloakframe_app::run::Folders;
use cloakframe_app::settings::{Method, Settings, Shape, Theme};
use cloakframe_app::tools::Tools;
use cloakframe_engine::session::StoredMedia;
use eframe::egui;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

mod support;
use support::RunTraced;

const WINDOW: egui::Vec2 = egui::vec2(1280.0, 800.0);

/// The window as the app's `eframe::App` draws it, edge to edge, which a harness built on a ui
/// closure does not: it puts a margin around the closure.
struct Window(AppState);

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
        support::record(ui.ctx());
    }
}

fn required_env(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{name} is not set (see CONTRIBUTING.md)"))
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

/// Twenty seconds of a photo panned and zoomed, so its faces move as in a street clip.
fn clip(photo: &Path, path: &Path) {
    let status = std::process::Command::new(required_env("CLOAKFRAME_FFMPEG"))
        .args(["-v", "error", "-loop", "1", "-i"])
        .arg(photo)
        .args([
            "-vf",
            "scale=1280:-2,zoompan=z='1.2+0.1*sin(on/60)':x='iw/2-iw/zoom/2+40*sin(on/45)':\
             y='ih/2-ih/zoom/2':d=1:s=1280x720:fps=30",
            "-t",
            "20",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}

fn screen<'a>(harness: &'a mut Harness<'_, Window>) -> &'a mut ReviewScreen {
    harness.state_mut().0.review.as_mut().unwrap()
}

struct Pictures {
    dir: PathBuf,
}

impl Pictures {
    /// The screen as it is now, in both themes and every language. `settle` is false where
    /// something keeps repainting (a run's progress), which a picture takes as it stands.
    fn take(&self, harness: &mut Harness<'_, Window>, screen: &str, settle: bool) {
        let language = harness.state().0.settings.language;
        harness.event(egui::Event::PointerGone);
        for (theme, theme_name) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
            for language in Language::ALL {
                harness.state_mut().0.command(Command::Theme(theme));
                harness
                    .state_mut()
                    .0
                    .command(Command::Language(Some(language)));
                if settle {
                    harness.run_traced();
                } else {
                    harness.step();
                    harness.step();
                }
                let name = format!("{screen}-{theme_name}-{language:?}.png").to_lowercase();
                harness.render().unwrap().save(self.dir.join(name)).unwrap();
            }
        }
        harness.state_mut().0.command(Command::Theme(Theme::Light));
        harness.state_mut().0.command(Command::Language(language));
        if settle {
            harness.run_traced();
        } else {
            harness.step();
        }
    }
}

/// Goes through the screens as a user would: the main screen empty and with files, the advanced
/// options, a run in progress and its result, the review queue, and the photo and video editors.
/// Needs the ONNX Runtime, the models, FFmpeg (CONTRIBUTING.md) and `CLOAKFRAME_SNAPSHOT_DIR`.
#[test]
#[ignore = "draws pictures to look at on a Mac"]
fn every_screen_in_both_themes_and_every_language() {
    let pictures = Pictures {
        dir: required_env("CLOAKFRAME_SNAPSHOT_DIR"),
    };
    std::fs::create_dir_all(&pictures.dir).unwrap();
    let assets = required_env("CLOAKFRAME_TEST_ASSETS");
    let work = std::env::temp_dir().join(format!("cloakframe-screens-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    let input = work.join("Downloads");
    std::fs::create_dir_all(&input).unwrap();
    for name in ["lena.jpg", "messi5.jpg", "astronaut.png", "car_image.webp"] {
        std::fs::copy(assets.join(name), input.join(name)).unwrap();
    }
    std::fs::write(input.join("notes.txt"), b"x").unwrap();
    clip(&assets.join("messi5.jpg"), &input.join("street.mp4"));

    let settings = Settings {
        language: Some(Language::Ko),
        output: Some(work.join("out")),
        plates: true,
        method: Method::Mosaic,
        ..Settings::default()
    };
    let mut harness = Harness::builder()
        .with_size(WINDOW)
        .with_pixels_per_point(2.0)
        .with_render_cursor(false)
        .wgpu()
        .with_context_setup(|ctx| ctx.set_fonts(fonts::definitions(Language::Ko)))
        .build_eframe(|_| Window(AppState::new(settings, None, Vec::new())));
    harness.state_mut().0.tools = Tools {
        ort_library: Some(required_env("CLOAKFRAME_ORT_LIBRARY")),
        ffmpeg: Some(required_env("CLOAKFRAME_FFMPEG")),
        ffprobe: Some(required_env("CLOAKFRAME_FFPROBE")),
    };
    harness.state_mut().0.set_folders(Folders {
        data: Some(work.join("data")),
        models: Some(assets),
        app_models: false,
    });
    harness.step();
    harness.state_mut().0.wait_for_sessions();
    harness.run_traced();
    pictures.take(&mut harness, "01-main", true);

    harness
        .input_mut()
        .dropped_files
        .push(Arc::new(Dropped(input.clone())));
    harness.step();
    harness.state_mut().0.wait_for_scan();
    harness.state_mut().0.wait_for_sessions();
    harness.step();
    let ctx = harness.ctx.clone();
    harness.state_mut().0.wait_for_thumbnails(&ctx);
    harness.run_traced();
    pictures.take(&mut harness, "02-main-ready", true);

    harness.state_mut().0.settings.shape = Shape::Ellipse;
    harness.run_traced();
    pictures.take(&mut harness, "12-shape-ellipse", true);
    harness.state_mut().0.settings.shape = Shape::Rectangle;

    let advanced = harness.state().0.tr().get("advanced-open");
    harness.get_by_label(&advanced).click();
    harness.run_traced();
    pictures.take(&mut harness, "03-advanced", true);
    let done = harness.state().0.tr().get("done");
    harness.get_by_role_and_label(Role::Button, &done).click();
    harness.run_traced();

    let start = harness.state().0.tr().get("start");
    harness.get_by_role_and_label(Role::Button, &start).click();
    harness.step();
    harness.state_mut().0.wait_for_run_items();
    harness.step();
    pictures.take(&mut harness, "04-run", false);

    harness.state_mut().0.wait_for_run();
    harness.step();
    harness.state_mut().0.wait_for_sessions();
    harness.run_traced();
    pictures.take(&mut harness, "05-result", true);

    let review = harness.state().0.tr().get("review-all");
    harness.get_by_role_and_label(Role::Button, &review).click();
    harness.step();
    screen(&mut harness).wait_for_thumbnails(&ctx);
    harness.run_traced();
    pictures.take(&mut harness, "06-queue", true);

    // The photo with the most detections, and the video.
    let busiest = |media: &StoredMedia| match media {
        StoredMedia::Photo { detections, .. } => detections.len(),
        StoredMedia::Video { .. } => usize::MAX,
    };
    let order = &harness.state().0.review.as_ref().unwrap().review.order;
    let photo = order
        .iter()
        .filter(|i| busiest(&i.media) != usize::MAX)
        .max_by_key(|i| busiest(&i.media))
        .unwrap()
        .item;
    let video = order
        .iter()
        .find(|i| busiest(&i.media) == usize::MAX)
        .unwrap()
        .item;
    for (item, name) in [(photo, "07-photo-editor"), (video, "08-video-editor")] {
        screen(&mut harness).open_item(item, &ctx);
        for _ in 0..2 {
            harness.step();
            screen(&mut harness).wait_for_picture(&ctx);
        }
        harness.run_traced();
        pictures.take(&mut harness, name, true);
        // The first box left unmasked and the next one selected, to see every kind of frame.
        for key in [egui::Key::Tab, egui::Key::Enter, egui::Key::Tab] {
            harness.key_press(key);
            harness.step();
            screen(&mut harness).wait_for_picture(&ctx);
        }
        harness.run_traced();
        pictures.take(&mut harness, &format!("{name}-picked"), true);
    }

    let _ = std::fs::remove_dir_all(&work);
}
