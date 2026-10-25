//! UI shared by the spike app (`main.rs`) and the egui_kittest tests.

use eframe::egui;
use egui::{Color32, ColorImage, TextureHandle, TextureOptions};

pub const KOREAN_SAMPLE: &str = "사진과 영상 속 얼굴·번호판을 가립니다. 다람쥐 헌 쳇바퀴에 타고파.";
pub const JAPANESE_SAMPLE: &str = "写真と動画の顔とナンバープレートを隠します。いろはにほへと ちりぬるを。";
pub const MIXED_SAMPLE: &str = "출력 폴더: ~/사진/익명화 — 出力フォルダー: ~/写真/匿名化";

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "noto_kr".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/NotoSansKR-Subset.otf")).into(),
    );
    fonts.font_data.insert(
        "noto_jp".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/NotoSansJP-Subset.otf")).into(),
    );
    // Korean before Japanese: the KR subset has no CJK ideographs, so kanji always take the
    // Japanese forms while Hangul and shared punctuation come from the Korean face.
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = fonts.families.entry(family).or_default();
        list.push("noto_kr".into());
        list.push("noto_jp".into());
    }
    ctx.set_fonts(fonts);
}

#[derive(Default)]
pub struct MenuLog {
    pub entries: Vec<String>,
}

#[derive(Default)]
pub struct SpikeState {
    pub name: String,
    pub note: String,
    pub dropped: Vec<String>,
    pub hovering: usize,
    pub menu: MenuLog,
    pub zoom: ZoomView,
}

/// A large image shown at a zoom factor around a centre point, drawn by cropping UVs so only
/// the visible part is sampled.
pub struct ZoomView {
    pub texture: Option<TextureHandle>,
    pub size: [usize; 2],
    pub zoom: f32,
    pub center: egui::Vec2,
}

impl Default for ZoomView {
    fn default() -> Self {
        Self { texture: None, size: [0, 0], zoom: 1.0, center: egui::vec2(0.5, 0.5) }
    }
}

impl ZoomView {
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let Some(texture) = &self.texture else {
            ui.label("4K 이미지 없음");
            return;
        };
        let available = ui.available_size();
        let aspect = self.size[0] as f32 / self.size[1] as f32;
        let mut size = egui::vec2(available.x, available.x / aspect);
        if size.y > available.y {
            size = egui::vec2(available.y * aspect, available.y);
        }
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        let response = response.on_hover_text("휠로 확대, 끌어서 이동");
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y + (i.zoom_delta() - 1.0) * 200.0);
            self.zoom = (self.zoom * (1.0 + scroll * 0.002)).clamp(1.0, 32.0);
        }
        if response.dragged() {
            let delta = response.drag_delta() / rect.size() / self.zoom;
            self.center -= delta;
        }
        let half = egui::vec2(0.5, 0.5) / self.zoom;
        self.center = self.center.clamp(half, egui::vec2(1.0, 1.0) - half);
        let uv = egui::Rect::from_min_max((self.center - half).to_pos2(), (self.center + half).to_pos2());
        ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "4K 미리보기"));
    }
}

/// 3840×2160 test card: fine lines and a grid so scaling artefacts are visible.
pub fn test_card(width: usize, height: usize) -> ColorImage {
    let mut pixels = vec![Color32::BLACK; width * height];
    for y in 0..height {
        for x in 0..width {
            let grid = x % 120 == 0 || y % 120 == 0;
            let fine = (x / 2 + y / 2) % 2 == 0 && (x / 480 + y / 480) % 2 == 0;
            pixels[y * width + x] = if grid {
                Color32::from_rgb(255, 210, 0)
            } else if fine {
                Color32::WHITE
            } else {
                Color32::from_rgb((x * 255 / width) as u8, (y * 255 / height) as u8, 96)
            };
        }
    }
    ColorImage::new([width, height], pixels)
}

pub fn load_test_card(ctx: &egui::Context, state: &mut SpikeState, width: usize, height: usize) {
    let image = test_card(width, height);
    state.zoom.size = [width, height];
    state.zoom.texture = Some(ctx.load_texture("test-card", image, TextureOptions::LINEAR));
}

pub fn spike_ui(ui: &mut egui::Ui, state: &mut SpikeState) {
    let (hovered, dropped) = ui.input(|i| (i.raw.hovered_files.len(), i.raw.dropped_files.clone()));
    state.hovering = hovered;
    for file in dropped {
        state.dropped.push(file.path().display().to_string());
    }

    egui::Panel::left("controls").resizable(true).default_size(420.0).show(ui, |ui| {
        ui.heading("CloakFrame egui spike");
        ui.label(KOREAN_SAMPLE);
        ui.label(JAPANESE_SAMPLE);
        ui.label(MIXED_SAMPLE);
        ui.separator();

        ui.horizontal(|ui| {
            let label = ui.label("이름");
            ui.add(egui::TextEdit::singleline(&mut state.name).hint_text("한글이나 日本語 입력")).labelled_by(label.id);
        });
        let label = ui.label("메모");
        ui.add(egui::TextEdit::multiline(&mut state.note).desired_rows(4)).labelled_by(label.id);
        ui.label(format!("이름 글자 수: {}", state.name.chars().count()));
        ui.separator();

        let drop_text = if state.hovering > 0 {
            format!("놓으면 추가: {}개", state.hovering)
        } else {
            "파일을 여기로 끌어 놓으세요".to_owned()
        };
        ui.add(egui::Button::new(drop_text).min_size(egui::vec2(ui.available_width(), 48.0)));
        ui.label(format!("받은 파일 {}개", state.dropped.len()));
        for name in state.dropped.iter().rev().take(5) {
            ui.label(name);
        }
        ui.separator();
        ui.label(format!("메뉴 이벤트 {}개", state.menu.entries.len()));
        for entry in state.menu.entries.iter().rev().take(5) {
            ui.label(entry);
        }
        if ui.button("확대 초기화").clicked() {
            state.zoom.zoom = 1.0;
            state.zoom.center = egui::vec2(0.5, 0.5);
        }
        ui.label(format!("확대 {:.1}×", state.zoom.zoom));
    });
    egui::CentralPanel::default().show(ui, |ui| state.zoom.ui(ui));
}
