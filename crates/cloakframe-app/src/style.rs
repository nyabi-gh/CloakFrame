//! The window's look (docs/M6_UX.md, visual system): one set of colours for light and one for
//! dark, sizes, and the few widgets egui does not draw this way. Hierarchy comes from size,
//! weight and grey alone; the one primary action of a screen is the only filled button.

use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle, Theme};

use crate::fonts;

pub const TOOLBAR_HEIGHT: f32 = 52.0;
pub const SIDEBAR_WIDTH: f32 = 220.0;
pub const INSPECTOR_WIDTH: f32 = 300.0;
pub const BUTTON_HEIGHT: f32 = 28.0;
pub const BUTTON_RADIUS: u8 = 6;
pub const THUMBNAIL_RADIUS: u8 = 4;

/// The colours of one theme, by role.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub window: Color32,
    pub sidebar: Color32,
    pub empty: Color32,
    pub canvas: Color32,
    pub separator: Color32,
    pub faint_line: Color32,
    pub border: Color32,
    pub text: Color32,
    /// Text a step below the main text, such as the empty screen's hint.
    pub body: Color32,
    pub secondary: Color32,
    pub disabled: Color32,
    pub selected: Color32,
    pub needs_review: Color32,
    pub saved: Color32,
    pub failed: Color32,
    /// The primary button's text, on `text`.
    pub on_primary: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub const LIGHT: Palette = Palette {
    window: rgb(0xFFFFFF),
    sidebar: rgb(0xF4F4F5),
    empty: rgb(0xFAFAFA),
    canvas: rgb(0xEDEDEF),
    separator: rgb(0xE4E4E7),
    faint_line: rgb(0xF0F0F1),
    border: rgb(0xD4D4D8),
    text: rgb(0x18181B),
    body: rgb(0x3F3F46),
    secondary: rgb(0x63636B),
    disabled: rgb(0x8E8E96),
    selected: rgb(0xE4E4E7),
    needs_review: rgb(0xC2410C),
    saved: rgb(0x15803D),
    failed: rgb(0xB91C1C),
    on_primary: rgb(0xFFFFFF),
};

pub const DARK: Palette = Palette {
    window: rgb(0x1F1F22),
    sidebar: rgb(0x19191C),
    empty: rgb(0x1B1B1E),
    canvas: rgb(0x111113),
    separator: rgb(0x2E2E33),
    faint_line: rgb(0x28282C),
    border: rgb(0x3A3A40),
    text: rgb(0xF4F4F5),
    body: rgb(0xD4D4D8),
    secondary: rgb(0xA1A1AA),
    disabled: rgb(0x71717A),
    selected: rgb(0x2E2E33),
    needs_review: rgb(0xFB923C),
    saved: rgb(0x4ADE80),
    failed: rgb(0xF87171),
    on_primary: rgb(0x18181B),
};

impl Palette {
    pub fn of(ui: &egui::Ui) -> &'static Self {
        if ui.visuals().dark_mode {
            &DARK
        } else {
            &LIGHT
        }
    }
}

fn visuals(theme: Theme, p: &Palette) -> egui::Visuals {
    let mut v = match theme {
        Theme::Light => egui::Visuals::light(),
        Theme::Dark => egui::Visuals::dark(),
    };
    let radius = CornerRadius::same(BUTTON_RADIUS);
    let widget = |fill: Color32, border: Color32| egui::style::WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: Stroke::new(1.0, border),
        corner_radius: radius,
        fg_stroke: Stroke::new(1.0, p.text),
        expansion: 0.0,
    };
    v.widgets.noninteractive = widget(p.window, p.separator);
    // bg_fill is the slider's rail and the checkbox's box; buttons fill with weak_bg_fill.
    v.widgets.inactive = egui::style::WidgetVisuals {
        bg_fill: p.separator,
        ..widget(p.window, p.border)
    };
    v.widgets.hovered = widget(p.sidebar, p.border);
    // Keyboard focus draws with these too, so the outline stands out.
    v.widgets.active = egui::style::WidgetVisuals {
        bg_stroke: Stroke::new(1.5, p.text),
        ..widget(p.selected, p.text)
    };
    v.widgets.open = widget(p.sidebar, p.border);
    v.selection.bg_fill = p.selected;
    v.selection.stroke = Stroke::new(1.0, p.text);
    v.hyperlink_color = p.text;
    v.panel_fill = p.window;
    v.window_fill = p.window;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.extreme_bg_color = p.sidebar;
    v.faint_bg_color = p.empty;
    v.code_bg_color = p.sidebar;
    v.weak_text_color = Some(p.secondary);
    v.warn_fg_color = p.needs_review;
    v.error_fg_color = p.failed;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = radius;
    v
}

/// Both themes' colours and the sizes they share; the theme setting picks between them.
pub fn install(ctx: &egui::Context) {
    ctx.set_visuals_of(Theme::Light, visuals(Theme::Light, &LIGHT));
    ctx.set_visuals_of(Theme::Dark, visuals(Theme::Dark, &DARK));
    ctx.all_styles_mut(|style| {
        let proportional = |size| FontId::proportional(size);
        style.text_styles = [
            (TextStyle::Small, proportional(12.0)),
            (TextStyle::Body, proportional(13.0)),
            (TextStyle::Button, proportional(13.0)),
            (TextStyle::Heading, FontId::new(14.0, fonts::medium())),
            (TextStyle::Monospace, FontId::monospace(12.0)),
        ]
        .into();
        let spacing = &mut style.spacing;
        spacing.item_spacing = egui::vec2(8.0, 6.0);
        spacing.button_padding = egui::vec2(12.0, 4.0);
        spacing.interact_size.y = BUTTON_HEIGHT;
    });
}

/// The width `text` takes in one line in the body font, or the primary button's.
pub fn text_width(ui: &egui::Ui, text: &str) -> f32 {
    egui::WidgetText::from(text)
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            TextStyle::Body,
        )
        .size()
        .x
}

/// The width of a button with `text`, for laying out a row before drawing it.
pub fn button_width(ui: &egui::Ui, text: &str, primary: bool) -> f32 {
    let text = if primary {
        egui::WidgetText::from(egui::RichText::new(text).font(medium(13.0)))
    } else {
        egui::WidgetText::from(text)
    };
    let width = text
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            TextStyle::Button,
        )
        .size()
        .x;
    width + 2.0 * ui.spacing().button_padding.x
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, fonts::medium())
}

/// A group's title in the settings panel and the sheet: small, bold, grey.
pub fn group_title(ui: &mut egui::Ui, text: &str) {
    let p = Palette::of(ui);
    ui.label(
        egui::RichText::new(text)
            .font(medium(12.0))
            .color(p.secondary),
    );
}

/// One group of the settings panel, with the faint line under it.
pub fn group<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let p = Palette::of(ui);
    let inner = egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(20, 16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 10.0;
            // Checkboxes and sliders take the line's height, not a button's.
            ui.spacing_mut().interact_size.y = 18.0;
            add(ui)
        });
    let rect = inner.response.rect;
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        Stroke::new(1.0, p.faint_line),
    );
    inner.inner
}

fn sized(button: egui::Button<'_>) -> egui::Button<'_> {
    button
        .min_size(egui::vec2(0.0, BUTTON_HEIGHT))
        .corner_radius(BUTTON_RADIUS)
}

/// The screen's one main action: filled with the text colour. When it cannot be pressed it is a
/// flat grey button, not a faded black one, so it does not read as pressable.
pub fn primary(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    let p = Palette::of(ui);
    let (fill, border, color) = if enabled {
        (p.text, p.text, p.on_primary)
    } else {
        (p.sidebar, p.separator, p.disabled)
    };
    let button = sized(egui::Button::new(
        egui::RichText::new(text).font(medium(13.0)).color(color),
    ))
    .fill(fill)
    .stroke(Stroke::new(1.0, border));
    let response = ui
        .scope(|ui| {
            ui.visuals_mut().disabled_alpha = 1.0;
            ui.add_enabled(enabled, button)
        })
        .inner;
    focus_ring(ui, &response);
    response
}

/// A button with a border, for actions beside the primary one.
pub fn outlined(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(sized(egui::Button::new(text)))
}

/// A button with no border until the pointer is on it, for the toolbar's secondary actions.
pub fn plain(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(sized(egui::Button::new(text).frame_when_inactive(false)))
}

/// Text that reads as a link and acts as a button.
pub fn link(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let p = Palette::of(ui);
    let response =
        ui.add(egui::Button::new(egui::RichText::new(text).color(p.text).underline()).frame(false));
    focus_ring(ui, &response);
    response
}

/// One choice of several, side by side in one bordered box, as the masking method is chosen.
/// Each part is a selectable button, so a screen reader hears which is chosen.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    value: &mut T,
    parts: &[(T, String)],
) -> bool {
    let p = Palette::of(ui);
    let before = *value;
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, BUTTON_HEIGHT), egui::Sense::hover());
    let part_width = width / parts.len() as f32;
    for (i, (choice, text)) in parts.iter().enumerate() {
        let part = egui::Rect::from_min_size(
            rect.min + egui::vec2(part_width * i as f32, 0.0),
            egui::vec2(part_width, BUTTON_HEIGHT),
        );
        let chosen = *value == *choice;
        let response = ui.interact(
            part,
            ui.make_persistent_id(("segment", i)),
            egui::Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::RadioButton, ui.is_enabled(), chosen, text)
        });
        if response.clicked() {
            *value = *choice;
        }
        let (fill, color) = if *value == *choice {
            (p.text, p.on_primary)
        } else if response.hovered() {
            (p.sidebar, p.body)
        } else {
            (p.window, p.body)
        };
        let corner = |left: bool, right: bool| CornerRadius {
            nw: if left { BUTTON_RADIUS } else { 0 },
            sw: if left { BUTTON_RADIUS } else { 0 },
            ne: if right { BUTTON_RADIUS } else { 0 },
            se: if right { BUTTON_RADIUS } else { 0 },
        };
        let radius = corner(i == 0, i + 1 == parts.len());
        ui.painter().rect_filled(part, radius, fill);
        ui.painter().text(
            part.center(),
            egui::Align2::CENTER_CENTER,
            text,
            FontId::proportional(13.0),
            color,
        );
        if i > 0 {
            ui.painter()
                .vline(part.left(), part.y_range(), Stroke::new(1.0, p.border));
        }
        focus_ring(ui, &response);
    }
    ui.painter().rect_stroke(
        rect,
        BUTTON_RADIUS,
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    *value != before
}

/// An entry of the sidebar: a row `height` tall that is filled when it is the screen shown.
pub fn sidebar_item(
    ui: &mut egui::Ui,
    selected: bool,
    label: &str,
    height: f32,
    add: impl FnOnce(&mut egui::Ui, &Palette),
) -> egui::Response {
    let p = Palette::of(ui);
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    let fill = if selected {
        p.selected
    } else if response.hovered() {
        p.faint_line
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, BUTTON_RADIUS, fill);
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(8.0, 4.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| add(ui, p),
    );
    focus_ring(ui, &response);
    response
}

/// The small round mark beside a state's words.
pub fn status_dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.5, color);
}

fn focus_ring(ui: &egui::Ui, response: &egui::Response) {
    if response.has_focus() {
        let p = Palette::of(ui);
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            BUTTON_RADIUS + 2,
            Stroke::new(1.5, p.text),
            egui::StrokeKind::Outside,
        );
    }
}

/// Icons drawn with the painter, so no icon font or image crate is needed. Each fills the square
/// `rect` in `color`.
pub mod icon {
    use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, vec2};

    fn at(rect: Rect, x: f32, y: f32) -> Pos2 {
        rect.min + vec2(x * rect.width(), y * rect.height())
    }

    fn arrow(painter: &Painter, rect: Rect, color: Color32, mirror: bool) {
        let x = |x: f32| if mirror { 1.0 - x } else { x };
        let stroke = Stroke::new(1.5, color);
        let arc: Vec<Pos2> = (0..=12)
            .map(|i| {
                let t = std::f32::consts::PI * (i as f32 / 12.0);
                at(rect, x(0.55 + 0.3 * t.sin()), 0.55 - 0.25 * t.cos())
            })
            .collect();
        painter.add(Shape::line(arc, stroke));
        painter.line_segment([at(rect, x(0.55), 0.3), at(rect, x(0.2), 0.3)], stroke);
        painter.add(Shape::line(
            vec![
                at(rect, x(0.35), 0.15),
                at(rect, x(0.2), 0.3),
                at(rect, x(0.35), 0.45),
            ],
            stroke,
        ));
    }

    pub fn undo(painter: &Painter, rect: Rect, color: Color32) {
        arrow(painter, rect, color, false);
    }

    pub fn redo(painter: &Painter, rect: Rect, color: Color32) {
        arrow(painter, rect, color, true);
    }

    pub fn play(painter: &Painter, rect: Rect, color: Color32) {
        painter.add(Shape::convex_polygon(
            vec![at(rect, 0.3, 0.2), at(rect, 0.8, 0.5), at(rect, 0.3, 0.8)],
            color,
            Stroke::NONE,
        ));
    }

    pub fn close(painter: &Painter, rect: Rect, color: Color32) {
        let stroke = Stroke::new(1.5, color);
        painter.line_segment([at(rect, 0.25, 0.25), at(rect, 0.75, 0.75)], stroke);
        painter.line_segment([at(rect, 0.75, 0.25), at(rect, 0.25, 0.75)], stroke);
    }

    /// A bin.
    pub fn discard(painter: &Painter, rect: Rect, color: Color32) {
        let stroke = Stroke::new(1.5, color);
        painter.line_segment([at(rect, 0.2, 0.28), at(rect, 0.8, 0.28)], stroke);
        painter.add(Shape::line(
            vec![
                at(rect, 0.4, 0.28),
                at(rect, 0.4, 0.16),
                at(rect, 0.6, 0.16),
                at(rect, 0.6, 0.28),
            ],
            stroke,
        ));
        painter.add(Shape::closed_line(
            vec![
                at(rect, 0.28, 0.28),
                at(rect, 0.32, 0.85),
                at(rect, 0.68, 0.85),
                at(rect, 0.72, 0.28),
            ],
            stroke,
        ));
    }
}
