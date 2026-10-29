//! What CloakFrame 1.12.1 left on this computer that the app takes over or removes. 1.12.1 named
//! its folders with Qt's standard locations for the organization and app "CloakFrame".

use std::path::PathBuf;

use crate::app_dirs;

/// Where 1.12.1 kept downloaded models (`<GenericDataLocation>/CloakFrame/models`), then where it
/// still looked for those of its predecessor Redactly. On macOS the first is
/// [`app_dirs::model_dir`] itself, so there 1.12.1's models are used where they are.
pub fn model_dirs() -> Vec<PathBuf> {
    app_dirs::data_home()
        .map(|home| {
            ["CloakFrame", "Redactly"]
                .iter()
                .map(|app| home.join(app).join("models"))
                .collect()
        })
        .unwrap_or_default()
}

/// 1.12.1's compiled CoreML models (`<CacheLocation>/coreml-models`, where the cache location is
/// `<cache_dir>/CloakFrame` for its organization and app name). The rewrite never reads them.
#[cfg(target_os = "macos")]
pub fn remove_coreml_cache(cache_dir: &std::path::Path) -> std::io::Result<bool> {
    match std::fs::remove_dir_all(cache_dir.join("CloakFrame/coreml-models")) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_folder_is_1_12_1s() {
        let dirs = model_dirs();
        let data = app_dirs::data_dir().unwrap();
        let home = data.parent().unwrap();
        assert_eq!(
            dirs,
            [home.join("CloakFrame/models"), home.join("Redactly/models")]
        );
        // Linux names the rewrite's folder in lower case, as XDG applications do, and Windows
        // keeps it out of 1.12.1's installation folder.
        let same = dirs[0] == app_dirs::model_dir().unwrap();
        assert_eq!(same, cfg!(target_os = "macos"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_coreml_cache_of_1_12_1_is_removed() {
        let dir = crate::test_support::TestDir::new();
        let compiled = dir.write("CloakFrame/coreml-models/abc/model.mlmodelc/coremldata.bin");
        let ours = dir.write("coreml/0123456789abcdef/model/x");
        assert!(remove_coreml_cache(dir.path()).unwrap());
        assert!(!compiled.parent().unwrap().exists());
        assert!(!dir.path().join("CloakFrame/coreml-models").exists());
        assert!(ours.exists());
        assert!(!remove_coreml_cache(dir.path()).unwrap());
    }
}
