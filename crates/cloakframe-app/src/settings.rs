//! `settings.toml` in the app's data folder. Keys this version does not know are kept as they
//! were, so a newer version's settings survive a run of this one.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::i18n::Language;

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    Fill,
    #[default]
    Mosaic,
    Blur,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    #[default]
    Rectangle,
    Ellipse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FaceModel {
    /// YOLO5Face-n.
    #[default]
    Accurate,
    /// YuNet.
    Fast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VideoQuality {
    #[default]
    High,
    Balanced,
    SpaceSaver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoCodec {
    #[default]
    H264,
    Hevc,
}

/// The score each step of the sensitivity slider masks from, least sensitive first; the middle
/// one is the models' default (plan 6: one slider, NMS is not shown).
pub const SENSITIVITY_SCORES: [f32; 5] = [0.70, 0.60, 0.50, 0.40, 0.30];

/// The settings folded away under the main screen's what and how. The defaults are what runs
/// used before there were any, so a session saved without them reads as it was run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Advanced {
    pub face_model: FaceModel,
    /// 1 to 5, a step of [`SENSITIVITY_SCORES`].
    pub sensitivity: u8,
    pub tiles: bool,
    /// The allowlisted camera and exposure tags of photos; nothing else is ever kept.
    pub keep_metadata: bool,
    /// The photo's location as well, only with `keep_metadata`.
    pub keep_gps: bool,
    pub remove_audio: bool,
    pub video_quality: VideoQuality,
    pub video_codec: VideoCodec,
    /// Detection on the GPU and the platform's hardware video encoders.
    pub gpu: bool,
}

impl Default for Advanced {
    fn default() -> Self {
        Self {
            face_model: FaceModel::default(),
            sensitivity: 3,
            tiles: true,
            keep_metadata: false,
            keep_gps: false,
            remove_audio: false,
            video_quality: VideoQuality::default(),
            video_codec: VideoCodec::default(),
            gpu: true,
        }
    }
}

impl Advanced {
    /// A value outside the slider, from a hand-edited file, counts as the nearest step.
    pub fn score(&self) -> f32 {
        let step = usize::from(self.sensitivity.clamp(1, 5)) - 1;
        SENSITIVITY_SCORES[step]
    }
}

/// The window's outer position and inner size, in points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowPlace {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    /// None follows the system.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
    pub theme: Theme,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowPlace>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<PathBuf>,
    pub faces: bool,
    pub plates: bool,
    pub method: Method,
    pub shape: Shape,
    pub mosaic_strength: u8,
    /// Hold every result for review; off publishes those with no unresolved problem.
    pub review: bool,
    pub advanced: Advanced,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            language: None,
            theme: Theme::default(),
            window: None,
            output: None,
            faces: true,
            plates: false,
            method: Method::default(),
            shape: Shape::default(),
            mosaic_strength: cloakframe_core::mask::MosaicStrength::default().cells(),
            review: true,
            advanced: Advanced::default(),
        }
    }
}

/// What reading the file found besides the settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// Dotted paths of keys this version does not know.
    UnknownKeys(Vec<String>),
    /// The file is not TOML this version can read; it is not written over.
    Unreadable(String),
    /// A newer version wrote it; it is read but not written over.
    Newer(u32),
}

pub struct SettingsFile {
    path: PathBuf,
    /// The file's own table, which saving updates instead of replacing.
    table: toml::Table,
    writable: bool,
}

impl SettingsFile {
    /// The settings at `path`; defaults when there is no file yet.
    pub fn load(path: impl Into<PathBuf>) -> (Self, Settings, Vec<Notice>) {
        let path = path.into();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Self::unreadable(path, e.to_string()),
        };
        let table: toml::Table = match text.parse() {
            Ok(table) => table,
            Err(e) => return Self::unreadable(path, e.message().to_owned()),
        };
        let settings: Settings = match toml::Value::Table(table.clone()).try_into() {
            Ok(settings) => settings,
            Err(e) => return Self::unreadable(path, e.message().to_owned()),
        };
        let mut notices = Vec::new();
        let known = toml::Table::try_from(Settings::default_with_every_key())
            .expect("settings serialize to a table");
        let unknown = unknown_keys(&table, &known, "");
        if !unknown.is_empty() {
            notices.push(Notice::UnknownKeys(unknown));
        }
        let writable = settings.version <= FORMAT_VERSION;
        if !writable {
            notices.push(Notice::Newer(settings.version));
        }
        let file = Self {
            path,
            table,
            writable,
        };
        (file, settings, notices)
    }

    fn unreadable(path: PathBuf, error: String) -> (Self, Settings, Vec<Notice>) {
        let file = Self {
            path,
            table: toml::Table::new(),
            writable: false,
        };
        (file, Settings::default(), vec![Notice::Unreadable(error)])
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes `settings` over the keys it has, keeping every other key the file had. Does nothing
    /// for a file this version must not write.
    pub fn save(&mut self, settings: &Settings) -> io::Result<()> {
        if !self.writable {
            return Ok(());
        }
        let ours = toml::Table::try_from(Settings {
            version: FORMAT_VERSION,
            ..settings.clone()
        })
        .map_err(io::Error::other)?;
        for key in Settings::optional_keys() {
            if !ours.contains_key(*key) {
                self.table.remove(*key);
            }
        }
        merge(&mut self.table, ours);
        let text = toml::to_string_pretty(&self.table).map_err(io::Error::other)?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = self.path.with_extension("toml.tmp");
        std::fs::write(&temp, text)?;
        std::fs::rename(&temp, &self.path)
    }
}

impl Settings {
    /// Every key filled in, for telling known keys from unknown ones.
    fn default_with_every_key() -> Self {
        Self {
            language: Some(Language::En),
            window: Some(WindowPlace {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            }),
            output: Some(PathBuf::new()),
            ..Self::default()
        }
    }

    /// Keys left out when unset, which saving must remove from the file.
    fn optional_keys() -> &'static [&'static str] {
        &["language", "window", "output"]
    }
}

fn unknown_keys(file: &toml::Table, known: &toml::Table, prefix: &str) -> Vec<String> {
    let mut unknown = Vec::new();
    for (key, value) in file {
        let path = format!("{prefix}{key}");
        match (value, known.get(key)) {
            (_, None) => unknown.push(path),
            (toml::Value::Table(inner), Some(toml::Value::Table(known))) => {
                unknown.extend(unknown_keys(inner, known, &format!("{path}.")));
            }
            _ => {}
        }
    }
    unknown
}

fn merge(into: &mut toml::Table, from: toml::Table) {
    for (key, value) in from {
        match (into.get_mut(&key), value) {
            (Some(toml::Value::Table(existing)), toml::Value::Table(inner)) => {
                merge(existing, inner)
            }
            (_, value) => {
                into.insert(key, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "cloakframe-app-settings-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn no_file_gives_the_defaults_and_saving_makes_one() {
        let dir = Dir::new("new");
        let path = dir.0.join("nested/settings.toml");
        let (mut file, settings, notices) = SettingsFile::load(&path);
        assert_eq!((settings.clone(), notices), (Settings::default(), vec![]));
        let changed = Settings {
            plates: true,
            language: Some(Language::Ja),
            ..settings
        };
        file.save(&changed).unwrap();
        let (_, back, notices) = SettingsFile::load(&path);
        assert_eq!((back, notices), (changed, vec![]));
    }

    #[test]
    fn advanced_settings_round_trip_in_their_own_table() {
        let dir = Dir::new("advanced");
        let path = dir.0.join("settings.toml");
        let (mut file, settings, _) = SettingsFile::load(&path);
        let changed = Settings {
            advanced: Advanced {
                face_model: FaceModel::Fast,
                sensitivity: 5,
                tiles: false,
                keep_metadata: true,
                keep_gps: true,
                remove_audio: true,
                video_quality: VideoQuality::SpaceSaver,
                video_codec: VideoCodec::Hevc,
                gpu: false,
            },
            ..settings
        };
        file.save(&changed).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let table: toml::Table = text.parse().unwrap();
        assert_eq!(
            table["advanced"]["video_quality"].as_str(),
            Some("space-saver")
        );
        assert_eq!(table["advanced"]["face_model"].as_str(), Some("fast"));
        let (_, back, notices) = SettingsFile::load(&path);
        assert_eq!((back, notices), (changed, vec![]));
    }

    #[test]
    fn shape_round_trips_and_is_a_rectangle_when_missing() {
        let dir = Dir::new("shape");
        let path = dir.0.join("settings.toml");
        std::fs::write(&path, "version = 1\nmethod = \"fill\"\n").unwrap();
        let (mut file, settings, notices) = SettingsFile::load(&path);
        assert!(notices.is_empty(), "{notices:?}");
        assert_eq!(settings.shape, Shape::Rectangle);
        let changed = Settings {
            shape: Shape::Ellipse,
            ..settings
        };
        file.save(&changed).unwrap();
        let table: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
        assert_eq!(table["shape"].as_str(), Some("ellipse"));
        let (_, back, notices) = SettingsFile::load(&path);
        assert_eq!((back, notices), (changed, vec![]));
    }

    /// A file from before the advanced settings, or one with only some of them, reads the rest
    /// as the defaults; a sensitivity off the slider counts as its nearest end.
    #[test]
    fn missing_advanced_settings_are_the_defaults() {
        let dir = Dir::new("partial");
        let path = dir.0.join("settings.toml");
        std::fs::write(
            &path,
            "version = 1\n[advanced]\ntiles = false\nsensitivity = 9\n",
        )
        .unwrap();
        let (_, settings, notices) = SettingsFile::load(&path);
        assert!(notices.is_empty(), "{notices:?}");
        assert_eq!(
            settings.advanced,
            Advanced {
                tiles: false,
                sensitivity: 9,
                ..Advanced::default()
            }
        );
        assert_eq!(settings.advanced.score(), 0.30);
        assert_eq!(Advanced::default().score(), 0.50);
    }

    #[test]
    fn unknown_keys_are_reported_and_kept() {
        let dir = Dir::new("unknown");
        let path = dir.0.join("settings.toml");
        std::fs::write(
            &path,
            "version = 1\nfaces = false\nfuture = 3\n\n[window]\nx = 1.0\ny = 2.0\nwidth = 3.0\nheight = 4.0\nmonitor = \"a\"\n",
        )
        .unwrap();
        let (mut file, settings, notices) = SettingsFile::load(&path);
        assert!(!settings.faces);
        assert_eq!(
            notices,
            [Notice::UnknownKeys(vec![
                "future".to_owned(),
                "window.monitor".to_owned()
            ])]
        );
        file.save(&Settings {
            window: Some(WindowPlace {
                x: 5.0,
                ..settings.window.unwrap()
            }),
            ..settings
        })
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let table: toml::Table = text.parse().unwrap();
        assert_eq!(table["future"].as_integer(), Some(3));
        assert_eq!(table["window"]["monitor"].as_str(), Some("a"));
        assert_eq!(table["window"]["x"].as_float(), Some(5.0));
    }

    #[test]
    fn unset_optional_keys_leave_the_file() {
        let dir = Dir::new("optional");
        let path = dir.0.join("settings.toml");
        std::fs::write(&path, "version = 1\nlanguage = \"ko\"\n").unwrap();
        let (mut file, settings, _) = SettingsFile::load(&path);
        file.save(&Settings {
            language: None,
            ..settings
        })
        .unwrap();
        let table: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
        assert!(!table.contains_key("language"));
    }

    #[test]
    fn unreadable_and_newer_files_are_not_written_over() {
        let dir = Dir::new("kept");
        let path = dir.0.join("settings.toml");
        for (text, notice) in [
            ("faces = [", None),
            ("faces = \"yes\"", None),
            ("version = 2\nfaces = false\n", Some(Notice::Newer(2))),
        ] {
            std::fs::write(&path, text).unwrap();
            let (mut file, settings, notices) = SettingsFile::load(&path);
            match notice {
                Some(n) => {
                    assert_eq!(notices, [n]);
                    assert!(!settings.faces);
                }
                None => assert!(
                    matches!(notices[..], [Notice::Unreadable(_)]),
                    "{notices:?}"
                ),
            }
            file.save(&Settings::default()).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        }
    }
}
