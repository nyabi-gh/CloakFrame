//! The window's look (docs/M6_UX.md, visual system): one set of colours for light and one for
//! dark, sizes, and the few widgets egui does not draw this way. Hierarchy comes from size,
//! weight and grey alone; the one primary action of a screen is the only filled button.

use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle, Theme};

use crate::fonts;

pub const TOOLBAR_HEIGHT: f32 = 52.0;
pub const SIDEBAR_WIDTH: f32 = 220.0;
pub const INSPECTOR_WIDTH: f32 = 300.0;
pub const BUTTON_HEIGHT: f32 = 28.0;
pub const SMALL_BUTTON_HEIGHT: f32 = 24.0;
pub const BUTTON_RADIUS: u8 = 6;
pub const THUMBNAIL_RADIUS: u8 = 4;
/// The narrowest a cell of a thumbnail grid gets before a column is dropped.
pub const GRID_MIN_CELL: f32 = 120.0;

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
    /// An uncertain detection's dashed frame on the canvas, brighter than the status dot so
    /// it shows on a photo.
    pub uncertain: Color32,
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
    uncertain: rgb(0xEA580C),
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
    uncertain: rgb(0xFB923C),
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

/// A checkbox as the design draws it: a ticked box is filled in the text colour, which egui's
/// own checkbox cannot do without its experimental theming.
pub struct Check<'a> {
    checked: &'a mut bool,
    text: egui::WidgetText,
}

impl<'a> Check<'a> {
    pub fn new(checked: &'a mut bool, text: impl Into<egui::WidgetText>) -> Self {
        Self {
            checked,
            text: text.into(),
        }
    }
}

impl egui::Widget for Check<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let spacing = ui.spacing();
        let (box_size, inner_size, gap) = (
            spacing.icon_width,
            spacing.icon_width_inner,
            spacing.icon_spacing,
        );
        let min_height = spacing.interact_size.y;
        let galley = self.text.into_galley(
            ui,
            None,
            ui.available_width() - box_size - gap,
            TextStyle::Button,
        );
        let size = egui::vec2(
            box_size + gap + galley.size().x,
            galley.size().y.max(min_height),
        );
        let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
        if response.clicked() {
            *self.checked = !*self.checked;
            response.mark_changed();
        }
        let checked = *self.checked;
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::Role::CheckBox,
                ui.is_enabled(),
                checked,
                galley.text(),
            )
        });
        if ui.is_rect_visible(rect) {
            let p = Palette::of(ui);
            let visuals = ui.style().interact(&response);
            let frame = egui::Rect::from_center_size(
                egui::pos2(rect.left() + box_size / 2.0, rect.center().y),
                egui::Vec2::splat(box_size),
            );
            let radius = visuals
                .corner_radius
                .at_most((0.3 * box_size).round() as u8);
            // An empty box is hollow: a grey fill reads as a box that cannot be ticked.
            let (fill, stroke) = if checked {
                (p.text, Stroke::new(1.0, p.text))
            } else if response.hovered() {
                (p.window, Stroke::new(1.0, p.secondary))
            } else {
                (p.window, Stroke::new(1.0, p.border))
            };
            let painter = ui.painter();
            painter.rect(frame, radius, fill, stroke, egui::StrokeKind::Inside);
            if checked {
                let tick =
                    egui::Rect::from_center_size(frame.center(), egui::Vec2::splat(inner_size));
                painter.line(
                    vec![
                        egui::pos2(tick.left(), tick.center().y),
                        egui::pos2(tick.center().x, tick.bottom()),
                        egui::pos2(tick.right(), tick.top()),
                    ],
                    Stroke::new(visuals.fg_stroke.width, p.on_primary),
                );
            }
            let text_pos = egui::pos2(frame.right() + gap, rect.center().y - galley.size().y / 2.0);
            painter.galley(text_pos, galley, visuals.text_color());
        }
        response
    }
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

/// A group's title in the settings panel and the advanced options: the body size in the main text colour
/// and Medium, so it stands above the grey row labels under it.
pub fn group_title(ui: &mut egui::Ui, text: &str) {
    let p = Palette::of(ui);
    ui.label(egui::RichText::new(text).font(medium(13.0)).color(p.text));
}

/// A row with its name in a column `label_width` wide and its controls beside it, so the
/// controls of every row start at one place even when a row has no name. Returns the name's
/// response.
pub fn field(
    ui: &mut egui::Ui,
    label_width: f32,
    label: &str,
    add: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let p = Palette::of(ui);
    ui.horizontal_top(|ui| {
        let name = ui
            .allocate_ui_with_layout(
                egui::vec2(label_width, BUTTON_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_size(egui::vec2(label_width, BUTTON_HEIGHT));
                    ui.label(egui::RichText::new(label).color(p.body))
                },
            )
            .inner;
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
        name
    })
    .inner
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

/// The screen's one main action: filled with the text colour. When it cannot be pressed it keeps
/// its colours at 40% opacity, so it still reads as the screen's main action.
pub fn primary(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    primary_sized(ui, text, enabled, egui::vec2(0.0, BUTTON_HEIGHT))
}

/// [`primary`] at least `size`, as the editor's approve button fills its row.
pub fn primary_sized(
    ui: &mut egui::Ui,
    text: &str,
    enabled: bool,
    size: egui::Vec2,
) -> egui::Response {
    let p = Palette::of(ui);
    let button = egui::Button::new(
        egui::RichText::new(text)
            .font(medium(13.0))
            .color(p.on_primary),
    )
    .min_size(size)
    .corner_radius(BUTTON_RADIUS)
    .fill(p.text)
    .stroke(Stroke::new(1.0, p.text));
    let response = ui
        .scope(|ui| {
            ui.visuals_mut().disabled_alpha = 0.4;
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

/// A row's own action, smaller than the toolbar's: bordered, or `plain` until the pointer is on
/// it.
pub fn small(ui: &mut egui::Ui, text: &str, plain: bool) -> egui::Response {
    let button = egui::Button::new(egui::RichText::new(text).size(12.0))
        .min_size(egui::vec2(0.0, SMALL_BUTTON_HEIGHT))
        .corner_radius(BUTTON_RADIUS)
        .frame_when_inactive(!plain)
        .wrap_mode(egui::TextWrapMode::Extend);
    // Not `ui.scope`: a child ui is placed before its size is known, so a wrapped row could not
    // move the button onto the next line.
    let padding = std::mem::replace(&mut ui.spacing_mut().button_padding, egui::vec2(8.0, 2.0));
    let response = ui.add(button);
    ui.spacing_mut().button_padding = padding;
    focus_ring(ui, &response);
    response
}

/// How wide [`small`] makes a button with `text`.
pub fn small_width(ui: &egui::Ui, text: &str) -> f32 {
    let width = ui.fonts_mut(|f| {
        f.layout_no_wrap(
            text.to_owned(),
            FontId::proportional(12.0),
            Color32::PLACEHOLDER,
        )
        .size()
        .x
    });
    width + 2.0 * 8.0
}

/// A square button that shows only `draw`'s icon; `label` is what a screen reader says and what
/// the pointer shows.
pub fn icon_button(
    ui: &mut egui::Ui,
    label: &str,
    draw: fn(&egui::Painter, egui::Rect, Color32),
) -> egui::Response {
    let p = Palette::of(ui);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(SMALL_BUTTON_HEIGHT, SMALL_BUTTON_HEIGHT),
        egui::Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    if response.hovered() {
        ui.painter().rect_filled(rect, 4, p.selected);
    }
    draw(ui.painter(), rect.shrink(6.0), p.secondary);
    focus_ring(ui, &response);
    response.on_hover_text(label)
}

/// An icon button of the toolbar's height, which shows itself greyed out while it cannot be
/// pressed.
pub fn toolbar_icon(
    ui: &mut egui::Ui,
    label: &str,
    enabled: bool,
    draw: fn(&egui::Painter, egui::Rect, Color32),
) -> egui::Response {
    let p = Palette::of(ui);
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(BUTTON_HEIGHT, BUTTON_HEIGHT), sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, BUTTON_RADIUS, p.selected);
    }
    let color = if enabled { p.body } else { p.disabled };
    draw(ui.painter(), rect.shrink(6.0), color);
    focus_ring(ui, &response);
    response.on_hover_text(label)
}

/// The part of a picture of `size` that fills `frame` without stretching, cut evenly at its
/// sides or its top and bottom.
pub fn cover(size: egui::Vec2, frame: egui::Vec2) -> egui::Rect {
    let (picture, wanted) = (size.x / size.y, frame.x / frame.y);
    if picture > wanted {
        let shown = wanted / picture;
        egui::Rect::from_min_max(
            egui::pos2((1.0 - shown) / 2.0, 0.0),
            egui::pos2((1.0 + shown) / 2.0, 1.0),
        )
    } else {
        let shown = picture / wanted;
        egui::Rect::from_min_max(
            egui::pos2(0.0, (1.0 - shown) / 2.0),
            egui::pos2(1.0, (1.0 + shown) / 2.0),
        )
    }
}

/// Text that reads as a link and acts as a button.
pub fn link(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let p = Palette::of(ui);
    let response =
        ui.add(egui::Button::new(egui::RichText::new(text).color(p.text).underline()).frame(false));
    focus_ring(ui, &response);
    response
}

/// One choice of several, side by side in equal parts of a recessed track, as the masking method
/// is chosen. The chosen part is a raised tab rather than a filled one, so it does not weigh as
/// much as the screen's primary button. Each part is a selectable button, so a screen reader
/// hears which is chosen.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    value: &mut T,
    parts: &[(T, String)],
) -> bool {
    const INSET: f32 = 2.0;
    let p = Palette::of(ui);
    let before = *value;
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, BUTTON_HEIGHT), egui::Sense::hover());
    ui.painter().rect_filled(rect, BUTTON_RADIUS, p.sidebar);
    let inner = rect.shrink(INSET);
    let part_width = inner.width() / parts.len() as f32;
    for (i, (choice, text)) in parts.iter().enumerate() {
        let part = egui::Rect::from_min_size(
            inner.min + egui::vec2(part_width * i as f32, 0.0),
            egui::vec2(part_width, inner.height()),
        );
        let response = ui.interact(
            part,
            ui.make_persistent_id(("segment", i)),
            egui::Sense::click(),
        );
        if response.clicked() {
            *value = *choice;
        }
        let chosen = *value == *choice;
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::RadioButton, ui.is_enabled(), chosen, text)
        });
        let radius = BUTTON_RADIUS - INSET as u8;
        let (font, color) = if chosen {
            ui.painter().rect(
                part,
                radius,
                p.window,
                Stroke::new(1.0, p.border),
                egui::StrokeKind::Inside,
            );
            (medium(13.0), p.text)
        } else if response.hovered() {
            (FontId::proportional(13.0), p.text)
        } else {
            (FontId::proportional(13.0), p.body)
        };
        ui.painter().text(
            part.center(),
            egui::Align2::CENTER_CENTER,
            text,
            font,
            color,
        );
        focus_ring(ui, &response);
    }
    *value != before
}

/// A whole-number slider `width` wide: a thin rail filled up to a hollow round handle, drawn as
/// the design does instead of egui's own, whose rail and handle share one fill. egui still
/// handles the dragging, the keys and what a screen reader can set.
pub fn slider(
    ui: &mut egui::Ui,
    value: &mut u8,
    range: std::ops::RangeInclusive<u8>,
    width: f32,
) -> egui::Response {
    const HEIGHT: f32 = 18.0;
    const RAIL: f32 = 4.0;
    let p = *Palette::of(ui);
    let rail = ui.painter().add(egui::Shape::Noop);
    let response = ui
        .scope(|ui| {
            let spacing = ui.spacing_mut();
            spacing.slider_width = width;
            spacing.interact_size.y = HEIGHT;
            spacing.slider_rail_height = 0.0;
            let handle = Stroke::new(2.0, p.text);
            let widgets = &mut ui.visuals_mut().widgets;
            for (state, expansion) in [
                (&mut widgets.inactive, 0.0),
                (&mut widgets.hovered, 1.0),
                (&mut widgets.active, 1.0),
            ] {
                state.bg_fill = p.window;
                state.fg_stroke = handle;
                state.expansion = expansion;
            }
            ui.add(
                egui::Slider::new(&mut *value, range.clone())
                    .show_value(false)
                    .trailing_fill(false)
                    .handle_shape(egui::style::HandleShape::Circle),
            )
        })
        .inner;
    let rect = response.rect;
    // Where egui puts the handle: its centre travels the rail inset by its radius.
    let travel = rect.x_range().shrink(rect.height() / 2.5);
    let (low, high) = (f32::from(*range.start()), f32::from(*range.end()));
    let at = egui::lerp(travel, (f32::from(*value) - low) / (high - low).max(1.0));
    let track = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), RAIL));
    let filled = egui::Rect::from_min_max(track.min, egui::pos2(at, track.max.y));
    ui.painter().set(
        rail,
        egui::Shape::Vec(vec![
            egui::Shape::rect_filled(track, RAIL / 2.0, p.separator),
            egui::Shape::rect_filled(filled, RAIL / 2.0, p.text),
        ]),
    );
    focus_ring(ui, &response);
    response
}

/// A drop-down list `width` wide whose arrow is a stroked chevron like the other icons, not
/// egui's filled triangle.
pub fn select<R>(
    ui: &mut egui::Ui,
    id: &str,
    width: f32,
    selected: impl Into<egui::WidgetText>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let p = *Palette::of(ui);
    egui::ComboBox::from_id_salt(id)
        .width(width)
        .selected_text(selected)
        .icon(move |ui, rect, _, _| {
            let rect = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(10.0));
            let at = |x: f32, y: f32| rect.min + egui::vec2(x * rect.width(), y * rect.height());
            ui.painter().add(egui::Shape::line(
                vec![at(0.2, 0.35), at(0.5, 0.65), at(0.8, 0.35)],
                Stroke::new(1.5, p.secondary),
            ));
        })
        .show_ui(ui, add)
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

/// The hollow mark of a state that has not begun, such as waiting.
pub fn status_ring(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
    ui.painter()
        .circle_stroke(rect.center(), 2.75, Stroke::new(1.5, color));
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

    fn chevron(painter: &Painter, rect: Rect, color: Color32, mirror: bool) {
        let x = |x: f32| if mirror { 1.0 - x } else { x };
        painter.add(Shape::line(
            vec![
                at(rect, x(0.62), 0.25),
                at(rect, x(0.38), 0.5),
                at(rect, x(0.62), 0.75),
            ],
            Stroke::new(1.5, color),
        ));
    }

    pub fn previous(painter: &Painter, rect: Rect, color: Color32) {
        chevron(painter, rect, color, false);
    }

    pub fn next(painter: &Painter, rect: Rect, color: Color32) {
        chevron(painter, rect, color, true);
    }

    pub fn play(painter: &Painter, rect: Rect, color: Color32) {
        painter.add(Shape::convex_polygon(
            vec![at(rect, 0.3, 0.2), at(rect, 0.8, 0.5), at(rect, 0.3, 0.8)],
            color,
            Stroke::NONE,
        ));
    }

    pub fn stop(painter: &Painter, rect: Rect, color: Color32) {
        painter.rect_filled(
            Rect::from_min_max(at(rect, 0.25, 0.25), at(rect, 0.75, 0.75)),
            1.0,
            color,
        );
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
