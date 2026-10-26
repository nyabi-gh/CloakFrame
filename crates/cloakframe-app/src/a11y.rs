//! Roles the app's lists need. `egui_extras::Table` gives no table, row or cell roles
//! (egui #8700), so every list is a container with the list role and rows with the item role,
//! named so a screen reader can tell them apart.

use eframe::egui::{self, accesskit::Role};

/// Gives the container being built its role and name. Its node has to exist before the widgets
/// inside it, which take the nearest node above them as their parent.
fn mark(ui: &egui::Ui, role: Role, label: &str) {
    ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
        node.set_role(role);
        node.set_label(label);
    });
}

/// Makes the container being built a list's row, for rows another container lays out, such as
/// a group's frame.
pub fn mark_item(ui: &egui::Ui, label: &str) {
    mark(ui, Role::ListItem, label);
}

pub fn list<R>(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope(|ui| {
        mark(ui, Role::List, label);
        add(ui)
    })
    .inner
}

/// A list's row, laid out left to right.
pub fn item<R>(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| {
        mark(ui, Role::ListItem, label);
        add(ui)
    })
    .inner
}

/// A list's row laid out top to bottom, such as a thumbnail over its name.
pub fn item_vertical<R>(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.vertical(|ui| {
        mark(ui, Role::ListItem, label);
        add(ui)
    })
    .inner
}
