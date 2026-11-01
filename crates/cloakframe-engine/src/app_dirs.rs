use std::ffi::OsString;
use std::path::PathBuf;

/// The per-user folder for app data such as review sessions, or `None` when the system names
/// none. Windows uses the local (not roaming) AppData, because sessions hold paths on this PC.
pub fn data_dir() -> Option<PathBuf> {
    platform_data_dir()
}

/// The per-user folder for files the app can make again, such as compiled CoreML models.
pub fn cache_dir() -> Option<PathBuf> {
    platform_cache_dir()
}

/// The folder for downloaded models. In the data folder rather than the cache, which cleaning
/// tools empty: each model was downloaded with the user's consent to its terms.
pub fn model_dir() -> Option<PathBuf> {
    data_dir().map(|data| data.join("models"))
}

/// Where results go until the user picks a folder: `CloakFrame` in the user's Pictures folder, or
/// in the home folder when there is none, as 1.12.1 did.
pub fn default_output_dir() -> Option<PathBuf> {
    platform_pictures_dir()
        .or_else(platform_home_dir)
        .map(|folder| folder.join("CloakFrame"))
}

/// The per-user folder applications keep their data under, which Qt calls GenericDataLocation.
pub(crate) fn data_home() -> Option<PathBuf> {
    platform_data_home()
}

fn absolute(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|p| p.is_absolute())
}

#[cfg(unix)]
fn platform_home_dir() -> Option<PathBuf> {
    absolute(std::env::var_os("HOME"))
}

#[cfg(target_os = "macos")]
fn platform_pictures_dir() -> Option<PathBuf> {
    platform_home_dir().map(|home| home.join("Pictures"))
}

#[cfg(target_os = "macos")]
fn platform_data_home() -> Option<PathBuf> {
    absolute(std::env::var_os("HOME")).map(|home| home.join("Library/Application Support"))
}

#[cfg(target_os = "macos")]
fn platform_data_dir() -> Option<PathBuf> {
    platform_data_home().map(|data| data.join("CloakFrame"))
}

#[cfg(target_os = "macos")]
fn platform_cache_dir() -> Option<PathBuf> {
    absolute(std::env::var_os("HOME")).map(|home| home.join("Library/Caches/CloakFrame"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_data_home() -> Option<PathBuf> {
    absolute(std::env::var_os("XDG_DATA_HOME"))
        .or_else(|| absolute(std::env::var_os("HOME")).map(|home| home.join(".local/share")))
}

/// `XDG_PICTURES_DIR` from xdg-user-dirs' `user-dirs.dirs`, or `~/Pictures` as Qt falls back to.
#[cfg(all(unix, not(target_os = "macos")))]
fn platform_pictures_dir() -> Option<PathBuf> {
    let home = platform_home_dir()?;
    let config =
        absolute(std::env::var_os("XDG_CONFIG_HOME")).unwrap_or_else(|| home.join(".config"));
    let named = std::fs::read_to_string(config.join("user-dirs.dirs"))
        .ok()
        .and_then(|text| xdg_pictures_dir(&text, &home));
    Some(named.unwrap_or_else(|| home.join("Pictures")))
}

/// The file's `XDG_PICTURES_DIR="$HOME/…"` or `="/…"` line; the format allows no other form. A
/// value of the home folder itself means the user turned the folder off.
#[cfg(all(unix, not(target_os = "macos")))]
fn xdg_pictures_dir(text: &str, home: &std::path::Path) -> Option<PathBuf> {
    let value = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("XDG_PICTURES_DIR="))
        .next_back()?
        .strip_prefix('"')?
        .strip_suffix('"')?;
    let path = if let Some(rest) = value.strip_prefix("$HOME") {
        let rest = rest.trim_start_matches('/');
        if rest.is_empty() {
            return None;
        }
        home.join(rest)
    } else {
        PathBuf::from(value)
    };
    path.is_absolute().then_some(path)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_data_dir() -> Option<PathBuf> {
    platform_data_home().map(|data| data.join("cloakframe"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_cache_dir() -> Option<PathBuf> {
    absolute(std::env::var_os("XDG_CACHE_HOME"))
        .or_else(|| absolute(std::env::var_os("HOME")).map(|home| home.join(".cache")))
        .map(|cache| cache.join("cloakframe"))
}

#[cfg(windows)]
fn platform_data_home() -> Option<PathBuf> {
    known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_LocalAppData)
}

#[cfg(windows)]
fn platform_pictures_dir() -> Option<PathBuf> {
    known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Pictures)
}

#[cfg(windows)]
fn platform_home_dir() -> Option<PathBuf> {
    known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Profile)
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn known_folder(id: &windows_sys::core::GUID) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::SHGetKnownFolderPath;

    let mut path = std::ptr::null_mut();
    // SAFETY: SHGetKnownFolderPath writes a NUL-terminated wide string that the caller frees with
    // CoTaskMemFree, also on failure; it is read before being freed.
    let folder = unsafe {
        let hr = SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut path);
        let folder = (hr >= 0 && !path.is_null()).then(|| {
            let len = (0..).take_while(|&i| *path.add(i) != 0).count();
            OsString::from_wide(std::slice::from_raw_parts(path, len))
        });
        CoTaskMemFree(path.cast());
        folder
    };
    absolute(folder)
}

/// Not `CloakFrame`, where Velopack installed 1.12.1, nor `CloakFrameApp`, 2.0's `packId`:
/// uninstalling either removes its whole folder, and the data must outlive both.
#[cfg(windows)]
fn platform_data_dir() -> Option<PathBuf> {
    platform_data_home().map(|local| local.join("CloakFrameData"))
}

#[cfg(windows)]
fn platform_cache_dir() -> Option<PathBuf> {
    platform_data_dir().map(|data| data.join("Cache"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_an_absolute_folder_for_cloakframe() {
        let dir = data_dir().expect("the test user has a data folder");
        assert!(dir.is_absolute());
        let app = |name: &std::ffi::OsStr| {
            name.eq_ignore_ascii_case("cloakframe") || name == "CloakFrameData"
        };
        assert!(app(dir.file_name().unwrap()), "{}", dir.display());
        let cache = cache_dir().expect("the test user has a cache folder");
        assert!(cache.is_absolute());
        assert!(
            cache.components().any(|c| app(c.as_os_str())),
            "{}",
            cache.display()
        );
        assert_ne!(cache, dir);
        assert_eq!(model_dir(), Some(dir.join("models")));
        assert_eq!(dir.parent(), data_home().as_deref());
    }

    /// Velopack installed 1.12.1 in `%LocalAppData%\CloakFrame` and installs 2.0 in
    /// `%LocalAppData%\CloakFrameApp`; uninstalling either removes its whole folder.
    #[cfg(windows)]
    #[test]
    fn windows_keeps_data_outside_both_installations() {
        let local = data_home().unwrap();
        let data = data_dir().unwrap();
        for installed in ["CloakFrame", "CloakFrameApp"] {
            assert!(
                !data.starts_with(local.join(installed)),
                "{}",
                data.display()
            );
        }
        assert_eq!(data, local.join("CloakFrameData"));
    }

    #[test]
    fn results_go_to_cloakframe_in_the_pictures_folder() {
        let output = default_output_dir().expect("the test user has a home folder");
        assert!(output.is_absolute());
        assert_eq!(output.file_name().unwrap(), "CloakFrame");
        assert_eq!(output.parent(), platform_pictures_dir().as_deref());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn reads_the_pictures_folder_xdg_user_dirs_names() {
        let home = std::path::Path::new("/home/me");
        let read = |text| xdg_pictures_dir(text, home);
        assert_eq!(
            read("# comment\nXDG_PICTURES_DIR=\"$HOME/Bilder\"\n"),
            Some(home.join("Bilder"))
        );
        assert_eq!(
            read("XDG_PICTURES_DIR=\"/data/photos\""),
            Some(PathBuf::from("/data/photos"))
        );
        assert_eq!(read("XDG_PICTURES_DIR=\"$HOME/\""), None);
        assert_eq!(read("XDG_PICTURES_DIR=\"photos\""), None);
        assert_eq!(read("XDG_DOCUMENTS_DIR=\"$HOME/Docs\""), None);
    }

    #[test]
    fn ignores_relative_environment_values() {
        assert_eq!(absolute(Some("relative/home".into())), None);
        assert_eq!(absolute(None), None);
    }
}
