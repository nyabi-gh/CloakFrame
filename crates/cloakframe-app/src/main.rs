// No console window behind the app on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use cloakframe_app::app::AppState;
use cloakframe_app::i18n::{Language, Translator};
use cloakframe_app::settings::{Notice, Settings, SettingsFile, WindowPlace};
use eframe::egui;
use fluent_bundle::FluentValue;

struct App {
    state: AppState,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    menu: Option<cloakframe_app::menu::native::NativeMenu>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    ctx: egui::Context,
    /// The window the menu is attached to on Windows.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    hwnd: Option<isize>,
    /// Where the window was last seen, saved when it closes.
    place: Option<WindowPlace>,
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(menu) = &self.menu {
            for command in menu.take(&mut raw.events) {
                self.state.command(command);
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = raw;
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let viewport = ui.ctx().input(|i| i.viewport().clone());
        if let (Some(outer), Some(inner)) = (viewport.outer_rect, viewport.inner_rect) {
            self.place = Some(WindowPlace {
                x: outer.min.x,
                y: outer.min.y,
                width: inner.width(),
                height: inner.height(),
            });
        }
        self.state.ui(ui);
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        self.follow_view_choices();
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.place.is_some() {
            self.state.settings.window = self.place;
            self.state.save_settings();
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl App {
    /// Keeps the View menu's ticks on the theme and language in use, and makes the menu again in
    /// a new language.
    fn follow_view_choices(&mut self) {
        let language = self.state.tr().language();
        if self.menu.as_ref().is_some_and(|m| m.language != language) {
            #[cfg(target_os = "windows")]
            if let (Some(old), Some(hwnd)) = (&self.menu, self.hwnd) {
                #[allow(unsafe_code)]
                // SAFETY: the window the old menu was attached to is still this app's.
                let removed = unsafe { old.menu.remove_for_hwnd(hwnd) };
                if let Err(e) = removed {
                    eprintln!("cannot remove the menu: {e}");
                }
            }
            self.menu = attach_menu(&self.ctx, &self.state, self.hwnd);
        }
        if let Some(menu) = &self.menu {
            menu.sync(self.state.settings.theme, self.state.settings.language);
        }
    }
}

#[cfg(target_os = "windows")]
mod accelerators {
    use std::sync::atomic::{AtomicIsize, Ordering};

    pub static TABLE: AtomicIsize = AtomicIsize::new(0);

    /// winit owns the message loop, so the menu's accelerators are translated in its hook.
    #[allow(unsafe_code)]
    pub fn hook(builder: &mut eframe::EventLoopBuilder<eframe::UserEvent>) {
        use winit::platform::windows::EventLoopBuilderExtWindows;
        builder.with_msg_hook(|msg| {
            use windows_sys::Win32::UI::WindowsAndMessaging::{MSG, TranslateAcceleratorW};
            let table = TABLE.load(Ordering::Relaxed);
            if table == 0 {
                return false;
            }
            let msg = msg as *const MSG;
            // SAFETY: winit hands the hook a valid MSG for the duration of the call, and the
            // table is the menu's, which lives as long as the app.
            unsafe { TranslateAcceleratorW((*msg).hwnd, table as _, msg) != 0 }
        });
    }
}

fn options(settings: &Settings, renderer: eframe::Renderer) -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("CloakFrame")
        .with_inner_size([1100.0, 760.0])
        .with_min_inner_size([640.0, 480.0]);
    if let Some(place) = settings.window {
        viewport = viewport
            .with_position([place.x, place.y])
            .with_inner_size([place.width, place.height]);
    }
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        viewport,
        renderer,
        ..Default::default()
    };
    #[cfg(target_os = "windows")]
    {
        options.event_loop_builder = Some(Box::new(accelerators::hook));
    }
    options
}

fn load_settings() -> (Option<SettingsFile>, Settings, Vec<Notice>) {
    match cloakframe_engine::app_dirs::data_dir() {
        Some(dir) => {
            let (file, settings, notices) = SettingsFile::load(dir.join("settings.toml"));
            (Some(file), settings, notices)
        }
        None => (None, Settings::default(), Vec::new()),
    }
}

fn run(renderer: eframe::Renderer) -> eframe::Result {
    let (file, settings, notices) = load_settings();
    let options = options(&settings, renderer);
    eframe::run_native(
        "CloakFrame",
        options,
        Box::new(move |cc| {
            let mut state = AppState::new(settings, file, notices).with_tools(
                cloakframe_app::tools::Tools::locate(),
                cloakframe_app::run::Folders::of_the_app(),
            );
            state.graphics = cloakframe_app::graphics::of(cc);
            if let Some(graphics) = &state.graphics {
                eprintln!("graphics: {graphics}");
            }
            state.install_fonts(&cc.egui_ctx);
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            let hwnd = window_handle(cc);
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            let menu = attach_menu(&cc.egui_ctx, &state, hwnd);
            Ok(Box::new(App {
                state,
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                menu,
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                ctx: cc.egui_ctx.clone(),
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                hwnd,
                place: None,
            }))
        }),
    )
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn window_handle(cc: &eframe::CreationContext<'_>) -> Option<isize> {
    #[cfg(target_os = "windows")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = cc.window_handle()
            && let RawWindowHandle::Win32(win) = handle.as_raw()
        {
            return Some(win.hwnd.get());
        }
    }
    let _ = cc;
    None
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn attach_menu(
    ctx: &egui::Context,
    state: &AppState,
    hwnd: Option<isize>,
) -> Option<cloakframe_app::menu::native::NativeMenu> {
    let menu = match cloakframe_app::menu::native::build(ctx, state.tr()) {
        Ok(menu) => menu,
        Err(e) => {
            eprintln!("cannot make the menu: {e}");
            return None;
        }
    };
    #[cfg(target_os = "macos")]
    {
        let _ = hwnd;
        menu.menu.init_for_nsapp();
    }
    #[cfg(target_os = "windows")]
    if let Some(hwnd) = hwnd {
        #[allow(unsafe_code)]
        // SAFETY: the handle is this app's window, which outlives the menu.
        let attached = unsafe { menu.menu.init_for_hwnd(hwnd) };
        match attached {
            Ok(()) => {
                accelerators::TABLE.store(menu.menu.haccel(), std::sync::atomic::Ordering::Relaxed)
            }
            Err(e) => eprintln!("cannot attach the menu: {e}"),
        }
    }
    Some(menu)
}

fn main() -> eframe::Result {
    // SAFETY: the first statement of main; no other thread exists yet.
    #[allow(unsafe_code)]
    unsafe {
        cloakframe_engine::detect::runtime::disable_telemetry()
    };
    #[cfg(target_os = "windows")]
    restrict_dll_search();
    // The installer and updater start the app with hook arguments and wait for it to exit.
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    velopack::VelopackApp::build().run();
    // Where wgpu finds no adapter (no GPU driver, some remote desktops), OpenGL often still runs.
    let wgpu = match run(eframe::Renderer::Wgpu) {
        Err(eframe::Error::Wgpu(e)) => e,
        other => return other,
    };
    eprintln!("wgpu could not start ({wgpu}); trying OpenGL");
    let result = run(eframe::Renderer::Glow);
    if let Err(opengl) = &result {
        // eframe's text already names the renderer each error comes from.
        cannot_draw(&format!("{}\n{opengl}", eframe::Error::Wgpu(wgpu)));
    }
    result
}

/// DLLs, ONNX Runtime's DirectML.dll among them, load from the app's folder and System32 only,
/// never from the current folder or PATH.
#[cfg(target_os = "windows")]
fn restrict_dll_search() {
    use windows_sys::Win32::System::LibraryLoader::{
        LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, SetDefaultDllDirectories,
    };
    #[allow(unsafe_code)]
    // SAFETY: takes only flags.
    let restricted = unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS) };
    if restricted == 0 {
        eprintln!(
            "cannot restrict the DLL search: {}",
            std::io::Error::last_os_error()
        );
    }
}

/// Without a window the app has nowhere else to say why it ends, and a release build on Windows
/// has no console either.
fn cannot_draw(errors: &str) {
    let language = load_settings()
        .1
        .language
        .unwrap_or_else(Language::of_system);
    let tr = Translator::new(language);
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(tr.get("graphics-failed-title"))
        .set_description(tr.with("graphics-failed", &[("errors", FluentValue::from(errors))]))
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}
