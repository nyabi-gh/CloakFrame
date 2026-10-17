use eframe::egui;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use egui_spike::{JAPANESE_SAMPLE, KOREAN_SAMPLE, SpikeState, install_fonts, load_test_card, spike_ui};

fn harness() -> Harness<'static, SpikeState> {
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, state: &mut SpikeState| {
            spike_ui(ui, state);
        },
        SpikeState::default(),
    );
    install_fonts(&harness.ctx);
    let ctx = harness.ctx.clone();
    load_test_card(&ctx, harness.state_mut(), 640, 360);
    harness.run();
    harness
}

fn ime(harness: &mut Harness<'_, SpikeState>, event: egui::ImeEvent) {
    harness.input_mut().events.push(egui::Event::Ime(event));
    harness.run();
}

fn preedit(text: &str) -> egui::ImeEvent {
    let len = text.chars().count();
    egui::ImeEvent::Preedit { text: text.to_owned(), active_range_chars: Some(len..len) }
}

#[test]
fn hangul_and_kana_have_glyphs() {
    let harness = harness();
    let font = egui::FontId::proportional(14.0);
    for sample in [KOREAN_SAMPLE, JAPANESE_SAMPLE, "ㄱㄴㄷㅏㅑ 뷁 똠방각하 繋がる"] {
        let missing: String = harness
            .ctx
            .fonts_mut(|f| sample.chars().filter(|c| !f.has_glyph(&font, *c)).collect());
        assert!(missing.is_empty(), "missing glyphs: {missing:?}");
    }
}

/// Name variants outside JIS X 0208 (IBM extensions and JIS X 0213). The subset does not cover
/// them; this records which ones fall back to nothing.
#[test]
fn name_variant_kanji_outside_the_subset() {
    let harness = harness();
    let font = egui::FontId::proportional(14.0);
    let variants = "髙﨑德瀨栁濵塚彅";
    let missing: String = harness.ctx.fonts_mut(|f| variants.chars().filter(|c| !f.has_glyph(&font, *c)).collect());
    eprintln!("name variants without a glyph: {missing}");
    assert!(!missing.is_empty(), "the subset now covers every listed variant; update the spike record");
}

/// The event order macOS's Korean 2-set IME produces through winit for "한글".
#[test]
fn korean_ime_composition_commits_each_syllable() {
    let mut harness = harness();
    harness.get_by_label("이름").focus();
    harness.run();
    for step in ["ㅎ", "하", "한"] {
        ime(&mut harness, preedit(step));
    }
    ime(&mut harness, egui::ImeEvent::Commit("한".into()));
    for step in ["ㄱ", "그", "글"] {
        ime(&mut harness, preedit(step));
    }
    // The preedit is shown in the field while composing.
    assert_eq!(harness.get_by_label("이름").value().as_deref(), Some("한글"));
    ime(&mut harness, egui::ImeEvent::Commit("글".into()));
    ime(&mut harness, preedit(""));
    assert_eq!(harness.state().name, "한글");
    assert_eq!(harness.state().name.chars().count(), 2);
}

/// Japanese conversion: kana preedit, then the converted kanji committed in one go.
#[test]
fn japanese_ime_conversion_commits_kanji() {
    let mut harness = harness();
    harness.get_by_label("이름").focus();
    harness.run();
    for step in ["に", "にほ", "にほん", "日本"] {
        ime(&mut harness, preedit(step));
    }
    ime(&mut harness, egui::ImeEvent::Commit("日本".into()));
    assert_eq!(harness.state().name, "日本");
}

/// Composition interrupted by a click elsewhere must not leave half a syllable behind.
#[test]
fn korean_ime_cancelled_preedit_leaves_nothing() {
    let mut harness = harness();
    harness.get_by_label("이름").focus();
    harness.run();
    ime(&mut harness, preedit("하"));
    ime(&mut harness, preedit(""));
    assert_eq!(harness.state().name, "");
}

#[test]
fn accessibility_tree_names_the_controls() {
    let harness = harness();
    harness.get_by_role_and_label(Role::TextInput, "이름");
    harness.get_by_role_and_label(Role::MultilineTextInput, "메모");
    harness.get_by_role_and_label(Role::Button, "확대 초기화");
    harness.get_by_label("4K 미리보기");
    harness.get_by_label_contains("파일을 여기로");
}

#[test]
fn dropped_files_are_listed() {
    let mut harness = harness();
    #[derive(Debug)]
    struct File(std::path::PathBuf);
    impl egui::DroppedFile for File {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            Err("unused".to_owned())
        }
    }
    harness.input_mut().dropped_files.push(std::sync::Arc::new(File("/tmp/사진/가족.jpg".into())));
    harness.run();
    assert_eq!(harness.state().dropped, vec!["/tmp/사진/가족.jpg".to_owned()]);
    harness.get_by_label("받은 파일 1개");
}

#[test]
fn cjk_text_renders() {
    let mut harness = harness();
    let image = harness.render().expect("wgpu test renderer");
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("spike-ui.png");
    image.save(&out).unwrap();
    eprintln!("rendered {}", out.display());
}
