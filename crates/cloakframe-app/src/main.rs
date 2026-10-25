// No console window behind the app on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use cloakframe_app::app::AppState;
use cloakframe_app::settings::{Settings, SettingsFile, WindowPlace};
use eframe::egui;

struct App {
    state: AppState,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    menu: Option<cloakframe_app::menu::native::NativeMenu>,
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
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.place.is_some() {
            self.state.settings.window = self.place;
            self.state.save_settings();
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

fn run(renderer: eframe::Renderer) -> eframe::Result {
    let (file, settings, notices) = match cloakframe_engine::app_dirs::data_dir() {
        Some(dir) => {
            let (file, settings, notices) = SettingsFile::load(dir.join("settings.toml"));
            (Some(file), settings, notices)
        }
        None => (None, Settings::default(), Vec::new()),
    };
    let options = options(&settings, renderer);
    eframe::run_native(
        "CloakFrame",
        options,
        Box::new(move |cc| {
            let mut state = AppState::new(settings, file, notices).with_tools(
                cloakframe_app::tools::Tools::locate(),
                cloakframe_app::run::Folders::of_the_app(),
            );
            state.install_fonts(&cc.egui_ctx);
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            let menu = native_menu(cc, &state);
            Ok(Box::new(App {
                state,
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                menu,
                place: None,
            }))
        }),
    )
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn native_menu(
    cc: &eframe::CreationContext<'_>,
    state: &AppState,
) -> Option<cloakframe_app::menu::native::NativeMenu> {
    let menu = match cloakframe_app::menu::native::build(&cc.egui_ctx, state.tr()) {
        Ok(menu) => menu,
        Err(e) => {
            eprintln!("cannot make the menu: {e}");
            return None;
        }
    };
    #[cfg(target_os = "macos")]
    menu.menu.init_for_nsapp();
    #[cfg(target_os = "windows")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = cc.window_handle()
            && let RawWindowHandle::Win32(win) = handle.as_raw()
        {
            #[allow(unsafe_code)]
            // SAFETY: the handle is this app's window, which outlives the menu.
            let attached = unsafe { menu.menu.init_for_hwnd(win.hwnd.get()) };
            match attached {
                Ok(()) => accelerators::TABLE
                    .store(menu.menu.haccel(), std::sync::atomic::Ordering::Relaxed),
                Err(e) => eprintln!("cannot attach the menu: {e}"),
            }
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
    // Where wgpu finds no adapter (no GPU driver, some remote desktops), OpenGL often still runs.
    match run(eframe::Renderer::Wgpu) {
        Err(eframe::Error::Wgpu(e)) => {
            eprintln!("wgpu could not start ({e}); trying OpenGL");
            run(eframe::Renderer::Glow)
        }
        other => other,
    }
}
