//! M0 egui spike app.
//!
//! egui-spike [--renderer wgpu|glow] [--no-vsync] [--menu predefined|custom|plain]
//!            [--bench upload|zoom] [--frames N] [--screenshot <png>]
//!
//! Without --bench it is an interactive window for the manual checks (IME, VoiceOver, file
//! drop, native menu). With --bench it runs N frames, prints JSON and exits.

use std::time::Instant;

use eframe::egui;
use egui_spike::{SpikeState, install_fonts, load_test_card, spike_ui};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Bench {
    Upload,
    Zoom,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MenuStyle {
    /// muda's predefined Edit items: AppKit/Win32 handle copy, paste, undo themselves.
    Predefined,
    /// Our own Edit items with the usual shortcuts; selecting one injects the egui event.
    Custom,
    /// Our own Edit items without shortcuts, so key presses always reach egui.
    Plain,
}

struct Args {
    renderer: eframe::Renderer,
    vsync: bool,
    menu: MenuStyle,
    bench: Option<Bench>,
    frames: usize,
    screenshot: Option<std::path::PathBuf>,
}

fn parse_args() -> Args {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    Args {
        renderer: match value("--renderer").as_deref() {
            Some("glow") => eframe::Renderer::Glow,
            _ => eframe::Renderer::Wgpu,
        },
        vsync: !args.iter().any(|a| a == "--no-vsync"),
        menu: match value("--menu").as_deref() {
            Some("custom") => MenuStyle::Custom,
            Some("plain") => MenuStyle::Plain,
            _ => MenuStyle::Predefined,
        },
        bench: match value("--bench").as_deref() {
            Some("upload") => Some(Bench::Upload),
            Some("zoom") => Some(Bench::Zoom),
            _ => None,
        },
        frames: value("--frames").and_then(|v| v.parse().ok()).unwrap_or(300),
        screenshot: value("--screenshot").map(Into::into),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native_menu {
    use super::MenuStyle;
    use eframe::egui;
    use muda::accelerator::{Accelerator, CMD_OR_CTRL, Code, Modifiers};
    use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

    pub struct NativeMenu {
        pub menu: Menu,
        pub rx: std::sync::mpsc::Receiver<MenuEvent>,
    }

    fn item(id: &str, text: &str, key: Option<(Modifiers, Code)>) -> MenuItem {
        MenuItem::with_id(id, text, true, key.map(|(m, c)| Accelerator::new(m, c)))
    }

    pub fn build(ctx: &egui::Context, style: MenuStyle) -> NativeMenu {
        let menu = Menu::new();
        #[cfg(target_os = "macos")]
        {
            let app = Submenu::new("CloakFrame", true);
            app.append_items(&[
                &PredefinedMenuItem::about(Some("CloakFrame 정보"), None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::services(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::quit(Some("CloakFrame 종료")),
            ])
            .unwrap();
            menu.append(&app).unwrap();
        }
        let file = Submenu::new("파일", true);
        file.append_items(&[
            &item("open", "파일 추가…", Some((CMD_OR_CTRL, Code::KeyO))),
            &item("export", "내보내기", Some((CMD_OR_CTRL | Modifiers::SHIFT, Code::KeyE))),
        ])
        .unwrap();
        #[cfg(target_os = "windows")]
        file.append_items(&[&PredefinedMenuItem::separator(), &PredefinedMenuItem::quit(Some("끝내기"))]).unwrap();
        menu.append(&file).unwrap();

        let edit = Submenu::new("편집", true);
        let shortcut = |code| (style == MenuStyle::Custom).then_some((CMD_OR_CTRL, code));
        match style {
            MenuStyle::Predefined => edit
                .append_items(&[
                    &PredefinedMenuItem::undo(Some("실행 취소")),
                    &PredefinedMenuItem::redo(Some("다시 실행")),
                    &PredefinedMenuItem::separator(),
                    &PredefinedMenuItem::cut(Some("오려두기")),
                    &PredefinedMenuItem::copy(Some("복사")),
                    &PredefinedMenuItem::paste(Some("붙여넣기")),
                    &PredefinedMenuItem::select_all(Some("전체 선택")),
                ])
                .unwrap(),
            MenuStyle::Custom | MenuStyle::Plain => edit
                .append_items(&[
                    &item("undo", "실행 취소", shortcut(Code::KeyZ)),
                    &item("cut", "오려두기", shortcut(Code::KeyX)),
                    &item("copy", "복사", shortcut(Code::KeyC)),
                    &item("paste", "붙여넣기", shortcut(Code::KeyV)),
                    &item("select_all", "전체 선택", shortcut(Code::KeyA)),
                ])
                .unwrap(),
        }
        menu.append(&edit).unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event| {
            let _ = tx.send(event);
            ctx.request_repaint();
        }));
        NativeMenu { menu, rx }
    }
}

struct App {
    state: SpikeState,
    bench: Option<Bench>,
    frames: usize,
    renderer: &'static str,
    started: Option<Instant>,
    last: Option<Instant>,
    intervals_ms: Vec<f64>,
    cpu_ms: Vec<f64>,
    convert_ms: Vec<f64>,
    set_ms: Vec<f64>,
    video: Option<egui::TextureHandle>,
    video_rgb: Vec<u8>,
    device: serde_json::Value,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    menu: Option<native_menu::NativeMenu>,
    screenshot: Option<std::path::PathBuf>,
    shown_frames: usize,
}

const VIDEO_W: usize = 1920;
const VIDEO_H: usize = 1080;

impl App {
    fn handle_menu(&mut self, ctx: &egui::Context) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(menu) = &self.menu {
            while let Ok(event) = menu.rx.try_recv() {
                let id = event.id().0.clone();
                self.state.menu.entries.push(id.clone());
                // Custom Edit items stand in for the shortcut, so turn them into the events
                // egui would have produced from the key press.
                let injected = match id.as_str() {
                    "copy" => Some(egui::Event::Copy),
                    "cut" => Some(egui::Event::Cut),
                    "paste" => arboard::Clipboard::new().and_then(|mut c| c.get_text()).ok().map(egui::Event::Paste),
                    "undo" => Some(egui::Event::Key {
                        key: egui::Key::Z,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::COMMAND,
                    }),
                    "select_all" => Some(egui::Event::Key {
                        key: egui::Key::A,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::COMMAND,
                    }),
                    _ => None,
                };
                if let Some(event) = injected {
                    ctx.input_mut(|i| i.events.push(event));
                }
            }
        }
    }

    fn bench_frame(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let now = Instant::now();
        if let Some(last) = self.last {
            self.intervals_ms.push((now - last).as_secs_f64() * 1000.0);
        }
        self.last = Some(now);
        self.started.get_or_insert(now);
        if let Some(cpu) = frame.info().cpu_usage {
            self.cpu_ms.push(cpu as f64 * 1000.0);
        }
        let n = self.intervals_ms.len();
        match self.bench {
            Some(Bench::Upload) => {
                // A moving pattern stands in for decoded BGR frames from the FFmpeg pipe.
                let shift = (n * 7) % VIDEO_W;
                for (i, px) in self.video_rgb.chunks_exact_mut(3).enumerate() {
                    let x = (i % VIDEO_W + shift) % VIDEO_W;
                    let y = i / VIDEO_W;
                    px[0] = (x * 255 / VIDEO_W) as u8;
                    px[1] = (y * 255 / VIDEO_H) as u8;
                    px[2] = ((x ^ y) & 0xFF) as u8;
                }
                let t = Instant::now();
                let image = egui::ColorImage::from_rgb([VIDEO_W, VIDEO_H], &self.video_rgb);
                self.convert_ms.push(t.elapsed().as_secs_f64() * 1000.0);
                let t = Instant::now();
                match &mut self.video {
                    Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
                    None => self.video = Some(ui.ctx().load_texture("video", image, egui::TextureOptions::LINEAR)),
                }
                self.set_ms.push(t.elapsed().as_secs_f64() * 1000.0);
                let tex = self.video.as_ref().unwrap();
                let size = ui.available_size();
                ui.image((tex.id(), egui::vec2(size.x, size.x * 9.0 / 16.0)));
            }
            Some(Bench::Zoom) => {
                let z = &mut self.state.zoom;
                z.zoom = (1.0 + n as f32 * 0.05).min(16.0);
                let t = n as f32 * 0.03;
                z.center = egui::vec2(0.5 + 0.3 * t.sin(), 0.5 + 0.2 * (t * 0.7).cos());
                spike_ui(ui, &mut self.state);
            }
            None => {}
        }
        ui.ctx().request_repaint();
        if n >= self.frames {
            self.report();
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn report(&self) {
        fn stats(v: &[f64]) -> serde_json::Value {
            if v.is_empty() {
                return serde_json::Value::Null;
            }
            let mut s = v.to_vec();
            s.sort_by(f64::total_cmp);
            let p = |q: f64| s[((s.len() - 1) as f64 * q).round() as usize];
            serde_json::json!({"n": s.len(), "p50": p(0.5), "p95": p(0.95), "max": s[s.len() - 1]})
        }
        // The first frames include texture creation and shader compilation.
        let skip = 10.min(self.intervals_ms.len());
        let steady = &self.intervals_ms[skip..];
        let total: f64 = steady.iter().sum();
        let report = serde_json::json!({
            "bench": format!("{:?}", self.bench.unwrap()),
            "renderer": self.renderer,
            "device": self.device,
            "frames": steady.len(),
            "fps": steady.len() as f64 / (total / 1000.0),
            "frame_interval_ms": stats(steady),
            "first_frames_ms": &self.intervals_ms[..skip],
            "cpu_frame_ms": stats(&self.cpu_ms[skip.min(self.cpu_ms.len())..]),
            "rgb_to_colorimage_ms": stats(&self.convert_ms),
            "texture_set_ms": stats(&self.set_ms),
        });
        println!("{report}");
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_menu(&ctx);
        if self.bench.is_some() {
            egui::CentralPanel::default().show(ui, |ui| self.bench_frame(ui, frame));
        } else {
            spike_ui(ui, &mut self.state);
            self.capture(&ctx);
        }
    }
}

impl App {
    /// Saves the window's own pixels once the UI has settled, then closes.
    fn capture(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        self.shown_frames += 1;
        if self.shown_frames == 30 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = shot {
            let [w, h] = image.size;
            let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            let header = format!("P7\nWIDTH {w}\nHEIGHT {h}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n");
            std::fs::write(&path, [header.as_bytes(), &bytes].concat()).unwrap();
            println!("saved {}x{} window capture to {}", w, h, path.display());
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if self.shown_frames < 300 {
            ctx.request_repaint();
        }
    }
}

fn device_info(cc: &eframe::CreationContext) -> serde_json::Value {
    if let Some(rs) = cc.wgpu_render_state.as_ref() {
        let info = rs.adapter.get_info();
        return serde_json::json!({
            "adapter": info.name,
            "backend": format!("{:?}", info.backend),
            "driver": info.driver_info,
            "max_texture_dimension_2d": rs.device.limits().max_texture_dimension_2d,
        });
    }
    if let Some(gl) = cc.gl.as_ref() {
        use eframe::glow::HasContext;
        let (renderer, version, max) = unsafe {
            (
                gl.get_parameter_string(eframe::glow::RENDERER),
                gl.get_parameter_string(eframe::glow::VERSION),
                gl.get_parameter_i32(eframe::glow::MAX_TEXTURE_SIZE),
            )
        };
        return serde_json::json!({"adapter": renderer, "backend": "OpenGL", "driver": version, "max_texture_dimension_2d": max});
    }
    serde_json::Value::Null
}

#[cfg(target_os = "windows")]
mod win_accel {
    use std::sync::atomic::{AtomicIsize, Ordering};
    pub static HACCEL: AtomicIsize = AtomicIsize::new(0);

    /// winit owns the message loop, so accelerators are translated in its message hook.
    pub fn hook(builder: &mut eframe::EventLoopBuilder<eframe::UserEvent>) {
        use winit::platform::windows::EventLoopBuilderExtWindows;
        builder.with_msg_hook(|msg| {
            use windows_sys::Win32::UI::WindowsAndMessaging::{MSG, TranslateAcceleratorW};
            let haccel = HACCEL.load(Ordering::Relaxed);
            if haccel == 0 {
                return false;
            }
            let msg = msg as *const MSG;
            unsafe { TranslateAcceleratorW((*msg).hwnd, haccel as _, msg) != 0 }
        });
    }
}

fn main() -> eframe::Result {
    let args = parse_args();
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1600.0, 1000.0]).with_title("CloakFrame egui spike"),
        renderer: args.renderer,
        ..Default::default()
    };
    options.glow_options.vsync = args.vsync;
    if !args.vsync {
        options.wgpu_options.surface.present_mode = eframe::wgpu::PresentMode::AutoNoVsync;
    }
    #[cfg(target_os = "windows")]
    {
        options.event_loop_builder = Some(Box::new(win_accel::hook));
    }
    let renderer = match args.renderer {
        eframe::Renderer::Glow => "glow",
        _ => "wgpu",
    };
    eframe::run_native(
        "CloakFrame egui spike",
        options,
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            let mut state = SpikeState::default();
            let t = Instant::now();
            load_test_card(&cc.egui_ctx, &mut state, 3840, 2160);
            eprintln!("4K test card built and queued in {:?}", t.elapsed());
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            let menu = if args.bench.is_none() {
                let menu = native_menu::build(&cc.egui_ctx, args.menu);
                #[cfg(target_os = "macos")]
                menu.menu.init_for_nsapp();
                #[cfg(target_os = "windows")]
                {
                    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                    if let Ok(handle) = cc.window_handle()
                        && let RawWindowHandle::Win32(win) = handle.as_raw()
                    {
                        unsafe { menu.menu.init_for_hwnd(win.hwnd.get()).unwrap() };
                        win_accel::HACCEL.store(menu.menu.haccel(), std::sync::atomic::Ordering::Relaxed);
                    }
                }
                Some(menu)
            } else {
                None
            };
            Ok(Box::new(App {
                state,
                bench: args.bench,
                frames: args.frames,
                renderer,
                started: None,
                last: None,
                intervals_ms: Vec::new(),
                cpu_ms: Vec::new(),
                convert_ms: Vec::new(),
                set_ms: Vec::new(),
                video: None,
                video_rgb: vec![0; VIDEO_W * VIDEO_H * 3],
                device: device_info(cc),
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                menu,
                screenshot: args.screenshot.clone(),
                shown_frames: 0,
            }))
        }),
    )
}
