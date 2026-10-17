//! D4: outputs and the folders made for them follow the umask and the folder's inherited ACL.
//! One test per binary, because the umask is process-wide.

use std::io::Write;
use std::path::Path;

use cloakframe_core::item::{Item, ItemId, PublishPermit, ReviewMode};
use cloakframe_engine::publish::TempOutput;

fn permit() -> PublishPermit {
    let mut item = Item::new(ItemId(1));
    item.finish_analysis().unwrap();
    item.route(ReviewMode::Off, &[]).unwrap();
    item.start_render().unwrap();
    item.mark_verified().unwrap();
    PublishPermit::issue(&item, &[]).unwrap()
}

fn publish(destination: &Path) {
    let mut out = TempOutput::create(destination).unwrap();
    out.file().write_all(b"pixels").unwrap();
    out.publish(&permit()).unwrap();
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cloakframe-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(unix)]
#[test]
fn outputs_follow_the_umask() {
    use rustix::fs::Mode;
    use rustix::process::umask;
    use std::os::unix::fs::PermissionsExt;

    let root = scratch("d4");
    let before = umask(Mode::from_raw_mode(0o022));
    for (mask, file, folder) in [
        (0o022, 0o644, 0o755),
        (0o002, 0o664, 0o775),
        (0o077, 0o600, 0o700),
    ] {
        umask(Mode::from_raw_mode(mask));
        let destination = root.join(format!("{mask:03o}/sub/photo.jpg"));
        publish(&destination);
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&destination), file, "file under umask {mask:03o}");
        assert_eq!(
            mode(destination.parent().unwrap()),
            folder,
            "folder under umask {mask:03o}"
        );
    }
    umask(before);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn outputs_inherit_the_folder_acl() {
    let root = scratch("d4");
    // S-1-5-32-545 is BUILTIN\Users; the SID keeps the check independent of the display language.
    let granted = std::process::Command::new("icacls")
        .arg(&root)
        .args(["/grant", "*S-1-5-32-545:(OI)(CI)R"])
        .status()
        .unwrap();
    assert!(granted.success());
    let destination = root.join("sub/photo.jpg");
    publish(&destination);
    let script = "param($p) $acl = Get-Acl -LiteralPath $p; \
        $users = $acl.Access | Where-Object { $_.IsInherited -and \
        $_.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value -eq 'S-1-5-32-545' }; \
        if ($acl.AreAccessRulesProtected -or -not $users) { exit 1 }";
    for path in [&destination, &destination.parent().unwrap().to_path_buf()] {
        let checked = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!("& {{ {script} }} '{}'", path.display()),
            ])
            .status()
            .unwrap();
        assert!(
            checked.success(),
            "{} does not inherit the folder ACL",
            path.display()
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
