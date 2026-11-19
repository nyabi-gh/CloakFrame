use cloakframe_engine::model_files::MODEL_FILES;
use eframe::egui;
use fluent_bundle::FluentValue;

use crate::app::AppState;
use crate::models;
use crate::style::{self, Palette};

const NOTICES: &str = include_str!("../../../THIRD_PARTY_NOTICES.txt");
const FONT_LICENSE: &str = include_str!("../assets/fonts/OFL.txt");

/// The app's license, the third-party notices and the models' terms, in a centered modal.
pub fn window(ctx: &egui::Context, state: &mut AppState) {
    if !state.about_open {
        return;
    }
    let p = *Palette::of_ctx(ctx);
    let tr = state.tr();
    let title = tr.get("about-title");
    let width = (ctx.content_rect().width() - 48.0).clamp(400.0, 620.0);
    let height = (ctx.content_rect().height() - 64.0).clamp(320.0, 560.0);
    let line = egui::Stroke::new(1.0, p.faint_line);
    let mut done = false;
    let modal = egui::Modal::new(egui::Id::unique("about"))
        .frame(style::dialog_frame(ctx))
        .backdrop_color(style::dialog_backdrop(ctx))
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.set_max_width(width);
            let top = ui.cursor().top();
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
                node.set_role(egui::accesskit::Role::Dialog);
            });
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(24, 12))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&title).font(style::medium(14.0)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let close = tr.get("close");
                            if style::icon_button(ui, &close, style::icon::close).clicked() {
                                done = true;
                            }
                        });
                    });
                });
            ui.painter()
                .hline(ui.max_rect().x_range(), ui.min_rect().bottom(), line);

            let rest = egui::Rect::from_x_y_ranges(
                ui.max_rect().x_range(),
                ui.cursor().top()..=top + height,
            );
            ui.advance_cursor_after_rect(rest);
            let (middle, footer) =
                rest.split_top_bottom_at_y(rest.bottom() - (style::BUTTON_HEIGHT + 28.0));
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(middle)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
                |ui| {
                    ui.set_clip_rect(middle.intersect(ui.clip_rect()));
                    egui::ScrollArea::vertical()
                        .id_salt("about-body")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Frame::NONE
                                .inner_margin(egui::Margin {
                                    left: 24,
                                    right: 24,
                                    top: 18,
                                    bottom: 18,
                                })
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.spacing_mut().item_spacing.y = 6.0;
                                    ui.label(
                                        egui::RichText::new(tr.get("app-name"))
                                            .font(style::medium(17.0))
                                            .color(p.text),
                                    );
                                    ui.label(
                                        egui::RichText::new(tr.with(
                                            "about-version",
                                            &[(
                                                "version",
                                                FluentValue::from(env!("CARGO_PKG_VERSION")),
                                            )],
                                        ))
                                        .color(p.body),
                                    );
                                    if let Some(graphics) = &state.graphics {
                                        ui.label(
                                            egui::RichText::new(tr.with(
                                                "about-graphics",
                                                &[(
                                                    "graphics",
                                                    FluentValue::from(graphics.as_str()),
                                                )],
                                            ))
                                            .size(12.0)
                                            .color(p.secondary),
                                        );
                                    }
                                    ui.label(
                                        egui::RichText::new(tr.get("about-license")).color(p.body),
                                    );

                                    ui.add_space(10.0);
                                    style::group_title(ui, &tr.get("about-models"));
                                    for file in &MODEL_FILES {
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(format!(
                                                    "{}: {}",
                                                    tr.get(models::name_id(file.model)),
                                                    tr.get(models::terms_id(file.model))
                                                ))
                                                .size(12.0)
                                                .color(p.secondary),
                                            )
                                            .wrap(),
                                        );
                                    }

                                    ui.add_space(10.0);
                                    style::group_title(ui, &tr.get("about-third-party"));
                                    egui::Frame::NONE
                                        .fill(p.sidebar)
                                        .stroke(egui::Stroke::new(1.0, p.separator))
                                        .corner_radius(style::BUTTON_RADIUS)
                                        .inner_margin(egui::Margin::symmetric(12, 10))
                                        .show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            egui::ScrollArea::vertical()
                                                .id_salt("about-notices")
                                                .max_height(200.0)
                                                .show(ui, |ui| {
                                                    ui.label(
                                                        egui::RichText::new(NOTICES)
                                                            .monospace()
                                                            .color(p.body),
                                                    );
                                                    ui.label(
                                                        egui::RichText::new(FONT_LICENSE)
                                                            .monospace()
                                                            .color(p.body),
                                                    );
                                                });
                                        });
                                });
                        });
                },
            );

            ui.painter()
                .hline(footer.x_range(), footer.top() + 0.5, line);
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(footer)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
                |ui| {
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(24, 14))
                        .show(ui, |ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), style::BUTTON_HEIGHT),
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if style::primary(ui, &tr.get("done"), true).clicked() {
                                        done = true;
                                    }
                                },
                            );
                        });
                },
            );
        });
    if done || modal.should_close() {
        state.about_open = false;
    }
}
