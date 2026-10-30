/// The update key's public half, base64 of 32 bytes (RELEASING, signing keys).
const UPDATE_KEY: &str = "CLOAKFRAME2_UPDATE_PUBLIC_KEY";

fn main() {
    println!("cargo::rerun-if-changed=../../packaging/windows/CloakFrame.manifest");
    println!("cargo::rerun-if-changed=../../assets/icons/cloakframe.ico");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    check_update_key(&target_os);
    if target_os != "windows" {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("../../assets/icons/cloakframe.ico")
        .set_manifest_file("../../packaging/windows/CloakFrame.manifest")
        .set("ProductName", "CloakFrame")
        .set("FileDescription", "CloakFrame");
    if let Err(e) = resource.compile() {
        panic!("cannot compile the Windows resources: {e}");
    }
}

/// A release build for Windows or Linux, where the app updates itself through Velopack, does not
/// build without the key: one that pinned none would never update (P-7).
fn check_update_key(target_os: &str) {
    println!("cargo::rerun-if-env-changed={UPDATE_KEY}");
    match std::env::var(UPDATE_KEY).ok().filter(|k| !k.is_empty()) {
        Some(key) => {
            let base64 = |b: u8| b.is_ascii_alphanumeric() || b == b'+' || b == b'/';
            let (body, pad) = key.as_bytes().split_at(key.len().min(43));
            if !(body.len() == 43 && body.iter().all(|&b| base64(b)) && pad == b"=") {
                panic!("{UPDATE_KEY} is not the base64 of a 32-byte Ed25519 public key");
            }
        }
        None => {
            let updates = matches!(target_os, "windows" | "linux");
            if updates && std::env::var("PROFILE").as_deref() == Ok("release") {
                panic!(
                    "a release build for {target_os} needs {UPDATE_KEY}, the public half of the \
                     update key; without it the app would never update"
                );
            }
        }
    }
}
