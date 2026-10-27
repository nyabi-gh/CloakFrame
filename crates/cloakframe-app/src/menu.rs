//! The menus: native on macOS and Windows (muda), an egui menu bar elsewhere, where muda only
//! attaches to GTK windows.

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use crate::app::{AppState, Command};
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use eframe::egui;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn bar(ui: &mut egui::Ui, state: &mut AppState) {
    egui::MenuBar::new().ui(ui, |ui| {
        let (file, add_files, add_folder, models, quit, help, about) = {
            let tr = state.tr();
            (
                tr.get("menu-file"),
                tr.get("add-files"),
                tr.get("add-folder"),
                tr.get("models-open"),
                tr.get("menu-file-quit"),
                tr.get("menu-help"),
                tr.get("menu-app-about"),
            )
        };
        ui.menu_button(file, |ui| {
            if ui.button(add_files).clicked() {
                state.command(Command::AddFiles);
            }
            if ui.button(add_folder).clicked() {
                state.command(Command::AddFolder);
            }
            if ui.button(models).clicked() {
                state.command(Command::Models);
            }
            ui.separator();
            if ui.button(quit).clicked() {
                state.command(Command::Quit);
            }
        });
        let (view, theme, language) = {
            let tr = state.tr();
            (tr.get("menu-view"), tr.get("theme"), tr.get("language"))
        };
        ui.menu_button(view, |ui| {
            ui.menu_button(theme, |ui| state.theme_choice(ui));
            ui.menu_button(language, |ui| state.language_choice(ui));
        });
        ui.menu_button(help, |ui| {
            if ui.button(about).clicked() {
                state.command(Command::About);
            }
        });
    });
}

/// Turns the native menu's choices into app commands and into the input events a shortcut would
/// have given: the Edit items carry the shortcuts, so the key presses reach the menu, not egui.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod native {
    use std::sync::mpsc;

    use eframe::egui;
    use muda::accelerator::{Accelerator, CMD_OR_CTRL, Code, Modifiers};
    use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

    use crate::app::Command;
    use crate::i18n::{Language, Translator};
    use crate::settings::Theme;

    pub struct NativeMenu {
        pub menu: Menu,
        events: mpsc::Receiver<MenuEvent>,
        /// The language its words are in; another language needs a new menu.
        pub language: Language,
        themes: Vec<(Theme, CheckMenuItem)>,
        languages: Vec<(Option<Language>, CheckMenuItem)>,
    }

    const THEMES: [(Theme, &str, &str); 3] = [
        (Theme::System, "theme:system", "theme-system"),
        (Theme::Light, "theme:light", "theme-light"),
        (Theme::Dark, "theme:dark", "theme-dark"),
    ];

    fn language_id(language: Option<Language>) -> String {
        format!("language:{language:?}")
    }

    fn item(id: &str, text: String, key: Option<(Modifiers, Code)>) -> MenuItem {
        MenuItem::with_id(id, text, true, key.map(|(m, c)| Accelerator::new(m, c)))
    }

    pub fn build(ctx: &egui::Context, tr: &Translator) -> Result<NativeMenu, muda::Error> {
        let menu = Menu::new();
        #[cfg(target_os = "macos")]
        {
            let app = Submenu::new(tr.get("app-name"), true);
            app.append_items(&[
                &item("about", tr.get("menu-app-about"), None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::services(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &item(
                    "quit",
                    tr.get("menu-app-quit"),
                    Some((CMD_OR_CTRL, Code::KeyQ)),
                ),
            ])?;
            menu.append(&app)?;
        }
        let file = Submenu::new(tr.get("menu-file"), true);
        file.append_items(&[
            &item(
                "add_files",
                tr.get("add-files"),
                Some((CMD_OR_CTRL, Code::KeyO)),
            ),
            &item(
                "add_folder",
                tr.get("add-folder"),
                Some((CMD_OR_CTRL | Modifiers::SHIFT, Code::KeyO)),
            ),
            &PredefinedMenuItem::separator(),
            &item("models", tr.get("models-open"), None),
        ])?;
        #[cfg(target_os = "windows")]
        file.append_items(&[
            &PredefinedMenuItem::separator(),
            &item("quit", tr.get("menu-file-quit"), None),
        ])?;
        menu.append(&file)?;

        let edit = Submenu::new(tr.get("menu-edit"), true);
        edit.append_items(&[
            &item(
                "undo",
                tr.get("menu-edit-undo"),
                Some((CMD_OR_CTRL, Code::KeyZ)),
            ),
            &item(
                "redo",
                tr.get("menu-edit-redo"),
                Some((CMD_OR_CTRL | Modifiers::SHIFT, Code::KeyZ)),
            ),
            &PredefinedMenuItem::separator(),
            &item(
                "cut",
                tr.get("menu-edit-cut"),
                Some((CMD_OR_CTRL, Code::KeyX)),
            ),
            &item(
                "copy",
                tr.get("menu-edit-copy"),
                Some((CMD_OR_CTRL, Code::KeyC)),
            ),
            &item(
                "paste",
                tr.get("menu-edit-paste"),
                Some((CMD_OR_CTRL, Code::KeyV)),
            ),
            &item(
                "select_all",
                tr.get("menu-edit-select-all"),
                Some((CMD_OR_CTRL, Code::KeyA)),
            ),
        ])?;
        menu.append(&edit)?;

        let check = |id: &str, text: String| CheckMenuItem::with_id(id, text, true, false, None);
        let themes: Vec<(Theme, CheckMenuItem)> = THEMES
            .iter()
            .map(|&(theme, id, text)| (theme, check(id, tr.get(text))))
            .collect();
        let languages: Vec<(Option<Language>, CheckMenuItem)> = std::iter::once(None)
            .chain(Language::ALL.map(Some))
            .map(|language| {
                let text = language.map_or_else(|| tr.get("language-system"), |l| l.name().into());
                (language, check(&language_id(language), text))
            })
            .collect();
        let theme = Submenu::new(tr.get("theme"), true);
        for (_, item) in &themes {
            theme.append(item)?;
        }
        let language = Submenu::new(tr.get("language"), true);
        for (_, item) in &languages {
            language.append(item)?;
        }
        let view = Submenu::new(tr.get("menu-view"), true);
        view.append_items(&[&theme, &language])?;
        menu.append(&view)?;
        #[cfg(target_os = "windows")]
        {
            let help = Submenu::new(tr.get("menu-help"), true);
            help.append(&item("about", tr.get("menu-app-about"), None))?;
            menu.append(&help)?;
        }

        let (send, events) = mpsc::channel();
        let repaint = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event| {
            let _ = send.send(event);
            repaint.request_repaint();
        }));
        Ok(NativeMenu {
            menu,
            events,
            language: tr.language(),
            themes,
            languages,
        })
    }

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    impl NativeMenu {
        /// Ticks the theme and the language in use. The menu ticks an item when it is chosen,
        /// so the others are unticked here.
        pub fn sync(&self, theme: Theme, language: Option<Language>) {
            for (t, item) in &self.themes {
                if item.is_checked() != (*t == theme) {
                    item.set_checked(*t == theme);
                }
            }
            for (l, item) in &self.languages {
                if item.is_checked() != (*l == language) {
                    item.set_checked(*l == language);
                }
            }
        }

        /// The commands chosen since the last call; edit choices go into `events`.
        pub fn take(&self, events: &mut Vec<egui::Event>) -> Vec<Command> {
            let mut commands = Vec::new();
            while let Ok(event) = self.events.try_recv() {
                match event.id().0.as_str() {
                    "about" => commands.push(Command::About),
                    "quit" => commands.push(Command::Quit),
                    "add_files" => commands.push(Command::AddFiles),
                    "add_folder" => commands.push(Command::AddFolder),
                    "models" => commands.push(Command::Models),
                    "copy" => events.push(egui::Event::Copy),
                    "cut" => events.push(egui::Event::Cut),
                    "paste" => {
                        if let Ok(text) = arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                            events.push(egui::Event::Paste(text));
                        }
                    }
                    "undo" => events.push(key(egui::Key::Z, egui::Modifiers::COMMAND)),
                    "redo" => events.push(key(
                        egui::Key::Z,
                        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    )),
                    "select_all" => events.push(key(egui::Key::A, egui::Modifiers::COMMAND)),
                    id => {
                        if let Some((theme, _)) = self.themes.iter().find(|(_, i)| i.id() == id) {
                            commands.push(Command::Theme(*theme));
                        } else if let Some((language, _)) =
                            self.languages.iter().find(|(_, i)| i.id() == id)
                        {
                            commands.push(Command::Language(*language));
                        }
                    }
                }
            }
            commands
        }
    }
}
