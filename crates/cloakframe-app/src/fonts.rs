//! Noto Sans KR/JP subsets after egui's own fonts (tools/fonts/subset.sh, SIL OFL 1.1).

use eframe::egui;

use crate::i18n::Language;

const KOREAN: &[u8] = include_bytes!("../assets/fonts/NotoSansKR-Subset.otf");
const JAPANESE: &[u8] = include_bytes!("../assets/fonts/NotoSansJP-Subset.otf");

/// Both subsets have CJK ideographs, in Korean and Japanese forms. The language the interface is
/// in decides which comes first; Hangul and kana each come from the only face that has them.
pub fn definitions(language: Language) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("noto_kr".into(), egui::FontData::from_static(KOREAN).into());
    fonts.font_data.insert(
        "noto_jp".into(),
        egui::FontData::from_static(JAPANESE).into(),
    );
    let order = match language {
        Language::Ko => ["noto_kr", "noto_jp"],
        Language::En | Language::Ja => ["noto_jp", "noto_kr"],
    };
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = fonts.families.entry(family).or_default();
        list.extend(order.map(String::from));
    }
    fonts
}
