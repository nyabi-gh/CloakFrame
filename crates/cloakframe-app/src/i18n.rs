//! The interface text in English, Korean and Japanese, from Fluent files. English is the source;
//! a message a translation lacks is shown in English.

use std::borrow::Cow;

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    En,
    Ko,
    Ja,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::En, Language::Ko, Language::Ja];

    /// The first of the system's preferred languages this app has, else English.
    pub fn of_system() -> Self {
        sys_locale::get_locales()
            .find_map(|tag| Self::of_tag(&tag))
            .unwrap_or(Self::En)
    }

    fn of_tag(tag: &str) -> Option<Self> {
        let primary = tag.split(['-', '_']).next()?.to_ascii_lowercase();
        match primary.as_str() {
            "en" => Some(Self::En),
            "ko" => Some(Self::Ko),
            "ja" => Some(Self::Ja),
            _ => None,
        }
    }

    /// Its own name, which the language menu shows whatever the current language.
    pub fn name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Ko => "한국어",
            Self::Ja => "日本語",
        }
    }

    fn source(self) -> &'static str {
        match self {
            Self::En => include_str!("../assets/i18n/en.ftl"),
            Self::Ko => include_str!("../assets/i18n/ko.ftl"),
            Self::Ja => include_str!("../assets/i18n/ja.ftl"),
        }
    }

    fn id(self) -> LanguageIdentifier {
        match self {
            Self::En => "en",
            Self::Ko => "ko",
            Self::Ja => "ja",
        }
        .parse()
        .expect("a valid language tag")
    }
}

pub struct Translator {
    language: Language,
    bundle: FluentBundle<FluentResource>,
    english: FluentBundle<FluentResource>,
}

fn bundle(language: Language) -> FluentBundle<FluentResource> {
    let resource = FluentResource::try_new(language.source().to_owned())
        .unwrap_or_else(|(_, errors)| panic!("{language:?} text does not parse: {errors:?}"));
    let mut bundle = FluentBundle::new(vec![language.id()]);
    // The isolation marks around arguments are invisible controls the bundled fonts cannot draw.
    bundle.set_use_isolating(false);
    bundle
        .add_resource(resource)
        .unwrap_or_else(|errors| panic!("{language:?} text repeats a message: {errors:?}"));
    bundle
}

impl Translator {
    pub fn new(language: Language) -> Self {
        Self {
            language,
            bundle: bundle(language),
            english: bundle(Language::En),
        }
    }

    pub fn language(&self) -> Language {
        self.language
    }

    pub fn get(&self, id: &str) -> String {
        self.format(id, None)
    }

    pub fn with(&self, id: &str, args: &[(&str, FluentValue<'_>)]) -> String {
        let mut fluent = FluentArgs::new();
        for (name, value) in args {
            fluent.set(*name, value.clone());
        }
        self.format(id, Some(&fluent))
    }

    fn format(&self, id: &str, args: Option<&FluentArgs<'_>>) -> String {
        for bundle in [&self.bundle, &self.english] {
            if let Some(pattern) = bundle.get_message(id).and_then(|m| m.value()) {
                let mut errors = Vec::new();
                let text: Cow<'_, str> = bundle.format_pattern(pattern, args, &mut errors);
                debug_assert!(errors.is_empty(), "{id}: {errors:?}");
                return text.into_owned();
            }
        }
        debug_assert!(false, "no message {id}");
        id.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(language: Language) -> Vec<String> {
        let resource = FluentResource::try_new(language.source().to_owned()).unwrap();
        let mut ids: Vec<String> = resource
            .entries()
            .filter_map(|e| match e {
                fluent_syntax::ast::Entry::Message(m) => Some(m.id.name.to_owned()),
                _ => None,
            })
            .collect();
        ids.sort();
        ids
    }

    #[test]
    fn every_translation_has_the_english_messages_and_no_others() {
        let english = ids(Language::En);
        for language in [Language::Ko, Language::Ja] {
            assert_eq!(ids(language), english, "{language:?}");
        }
    }

    #[test]
    fn arguments_and_plurals_format() {
        let en = Translator::new(Language::En);
        let one = en.with("drop-release", &[("count", 1.into())]);
        assert_eq!(one, "Release to add one item");
        let three = en.with("drop-release", &[("count", 3.into())]);
        assert_eq!(three, "Release to add 3 items");
        let ko = Translator::new(Language::Ko);
        assert_eq!(
            ko.with("drop-release", &[("count", 3.into())]),
            "놓으면 3개 추가"
        );
    }

    #[test]
    fn system_tags_map_to_the_languages_there_are() {
        assert_eq!(Language::of_tag("ko-KR"), Some(Language::Ko));
        assert_eq!(Language::of_tag("ja_JP"), Some(Language::Ja));
        assert_eq!(Language::of_tag("en-GB"), Some(Language::En));
        assert_eq!(Language::of_tag("de-DE"), None);
    }
}
