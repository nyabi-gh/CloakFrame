//! Noto Sans KR/JP subsets after egui's own fonts (tools/fonts/subset.sh, SIL OFL 1.1).

use eframe::egui;

use crate::i18n::Language;

const KOREAN: &[u8] = include_bytes!("../assets/fonts/NotoSansKR-Subset.otf");
const JAPANESE: &[u8] = include_bytes!("../assets/fonts/NotoSansJP-Subset.otf");
const KOREAN_MEDIUM: &[u8] = include_bytes!("../assets/fonts/NotoSansKR-Medium-Subset.otf");
const JAPANESE_MEDIUM: &[u8] = include_bytes!("../assets/fonts/NotoSansJP-Medium-Subset.otf");

/// The family of the Medium faces. egui does not make bold text from a regular face, so titles
/// and the primary button need a heavier face of their own.
pub fn medium() -> egui::FontFamily {
    egui::FontFamily::Name("medium".into())
}

/// Both subsets have CJK ideographs, in Korean and Japanese forms. The language the interface is
/// in decides which comes first; Hangul and kana each come from the only face that has them.
pub fn definitions(language: Language) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    for (name, data) in [
        ("noto_kr", KOREAN),
        ("noto_jp", JAPANESE),
        ("noto_kr_medium", KOREAN_MEDIUM),
        ("noto_jp_medium", JAPANESE_MEDIUM),
    ] {
        fonts
            .font_data
            .insert(name.into(), egui::FontData::from_static(data).into());
    }
    let (first, second) = match language {
        Language::Ko => ("kr", "jp"),
        Language::En | Language::Ja => ("jp", "kr"),
    };
    let regular = [format!("noto_{first}"), format!("noto_{second}")];
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = fonts.families.entry(family).or_default();
        list.extend(regular.iter().cloned());
    }
    fonts.families.insert(
        medium(),
        vec![
            format!("noto_{first}_medium"),
            format!("noto_{second}_medium"),
        ],
    );
    fonts
}
