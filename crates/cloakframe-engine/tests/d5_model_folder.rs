//! D5: a model is saved whatever the permissions of the model folder; 1.12.1 refused a folder
//! others could write to, so under umask 002 it saved nothing. What keeps a changed file from
//! running is the hash check when the model is loaded (cli_photo.rs only_the_pinned_models_run).
//! One test per binary, because the umask is process-wide.
//!
//! Needs `CLOAKFRAME_TEST_ASSETS` for a pinned model file.

mod common;

use std::path::{Path, PathBuf};

use cloakframe_core::detection::ModelId;
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::model_files::{ModelStore, Status, model_file};
use common::required_env;

fn save(dir: &Path) {
    let bytes =
        std::fs::read(required_env("CLOAKFRAME_TEST_ASSETS").join(model_file(ModelId::YuNet).name))
            .unwrap();
    let store = ModelStore::new(dir);
    store
        .install(
            ModelId::YuNet,
            &mut &bytes[..],
            &CancelToken::new(),
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(store.status(ModelId::YuNet).unwrap(), Status::Present);
}

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cloakframe-d5-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(unix)]
#[test]
fn models_are_saved_under_any_umask() {
    use rustix::fs::Mode;
    use rustix::process::umask;
    use std::os::unix::fs::PermissionsExt;

    let root = scratch();
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let before = umask(Mode::from_raw_mode(0o022));
    for (mask, file, folder) in [(0o022, 0o644, 0o755), (0o002, 0o664, 0o775)] {
        umask(Mode::from_raw_mode(mask));
        let dir = root.join(format!("{mask:03o}/models"));
        save(&dir);
        assert_eq!(mode(&dir), folder, "folder under umask {mask:03o}");
        let saved = dir.join(model_file(ModelId::YuNet).name);
        assert_eq!(mode(&saved), file, "file under umask {mask:03o}");
    }
    umask(before);

    let open = root.join("open");
    std::fs::create_dir(&open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
    save(&open);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn models_are_saved_in_a_folder_others_can_change() {
    let root = scratch();
    // S-1-5-32-545 is BUILTIN\Users; the SID keeps the check independent of the display language.
    let granted = std::process::Command::new("icacls")
        .arg(&root)
        .args(["/grant", "*S-1-5-32-545:(OI)(CI)M"])
        .status()
        .unwrap();
    assert!(granted.success());
    save(&root.join("models"));
    std::fs::remove_dir_all(root).unwrap();
}
