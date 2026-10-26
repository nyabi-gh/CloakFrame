use cloakframe_engine::model_files::MODEL_FILES;
use eframe::egui;
use fluent_bundle::FluentValue;

use crate::app::AppState;

const NOTICES: &str = include_str!("../../../THIRD_PARTY_NOTICES.txt");
const FONT_LICENSE: &str = include_str!("../assets/fonts/OFL.txt");

/// The app's license, the third-party notices and the models' terms, in a window the user opens.
pub fn window(ctx: &egui::Context, state: &mut AppState) {
    let mut open = state.about_open;
    let tr = state.tr();
    egui::Window::new(tr.get("about-title"))
        .open(&mut open)
        .default_size([640.0, 480.0])
        .show(ctx, |ui| {
            ui.heading(tr.get("app-name"));
            ui.label(tr.with(
                "about-version",
                &[("version", FluentValue::from(env!("CARGO_PKG_VERSION")))],
            ));
            if let Some(graphics) = &state.graphics {
                ui.label(tr.with(
                    "about-graphics",
                    &[("graphics", FluentValue::from(graphics.as_str()))],
                ));
            }
            ui.label(tr.get("about-license"));
            ui.separator();
            ui.strong(tr.get("about-models"));
            for file in &MODEL_FILES {
                ui.label(format!("{}: {}", file.name, file.terms.summary));
            }
            ui.separator();
            ui.strong(tr.get("about-third-party"));
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    ui.monospace(NOTICES);
                    ui.monospace(FONT_LICENSE);
                });
        });
    state.about_open = open;
}
