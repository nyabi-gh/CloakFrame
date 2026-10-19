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

fn absolute(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|p| p.is_absolute())
}

#[cfg(target_os = "macos")]
fn platform_data_dir() -> Option<PathBuf> {
    absolute(std::env::var_os("HOME"))
        .map(|home| home.join("Library/Application Support/CloakFrame"))
}

#[cfg(target_os = "macos")]
fn platform_cache_dir() -> Option<PathBuf> {
    absolute(std::env::var_os("HOME")).map(|home| home.join("Library/Caches/CloakFrame"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_data_dir() -> Option<PathBuf> {
    absolute(std::env::var_os("XDG_DATA_HOME"))
        .or_else(|| absolute(std::env::var_os("HOME")).map(|home| home.join(".local/share")))
        .map(|data| data.join("cloakframe"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_cache_dir() -> Option<PathBuf> {
    absolute(std::env::var_os("XDG_CACHE_HOME"))
        .or_else(|| absolute(std::env::var_os("HOME")).map(|home| home.join(".cache")))
        .map(|cache| cache.join("cloakframe"))
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn platform_data_dir() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath};

    let mut path = std::ptr::null_mut();
    // SAFETY: SHGetKnownFolderPath writes a NUL-terminated wide string that the caller frees with
    // CoTaskMemFree, also on failure; it is read before being freed.
    let folder = unsafe {
        let hr = SHGetKnownFolderPath(&FOLDERID_LocalAppData, 0, std::ptr::null_mut(), &mut path);
        let folder = (hr >= 0 && !path.is_null()).then(|| {
            let len = (0..).take_while(|&i| *path.add(i) != 0).count();
            OsString::from_wide(std::slice::from_raw_parts(path, len))
        });
        CoTaskMemFree(path.cast());
        folder
    };
    absolute(folder).map(|local| local.join("CloakFrame"))
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
        assert!(dir.ends_with("CloakFrame") || dir.ends_with("cloakframe"));
        let cache = cache_dir().expect("the test user has a cache folder");
        assert!(cache.is_absolute());
        let app = |c: std::path::Component<'_>| c.as_os_str().eq_ignore_ascii_case("cloakframe");
        assert!(cache.components().any(app), "{}", cache.display());
        assert_ne!(cache, dir);
    }

    #[test]
    fn ignores_relative_environment_values() {
        assert_eq!(absolute(Some("relative/home".into())), None);
        assert_eq!(absolute(None), None);
    }
}
