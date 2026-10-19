//! Windows on FAT32 (camera SD cards): files are scanned with distinct identities and outputs are
//! published without replacing anything. FAT32 has no 128-bit file IDs, so identity falls back to
//! the file index (review item 16).
//!
//! Needs `CLOAKFRAME_FAT32_DIR`, a folder on a FAT32 volume; CI makes one from a VHD
//! (`.github/workflows/ci.yml`).
#![cfg(windows)]

use std::io::Write;
use std::path::PathBuf;

use cloakframe_core::item::{Item, ItemId, PublishPermit, ReviewMode};
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::identity::identity;
use cloakframe_engine::publish::TempOutput;
use cloakframe_engine::scan::{ScanRequest, scan};

fn permit() -> PublishPermit {
    let mut item = Item::new(ItemId(1));
    item.finish_analysis().unwrap();
    item.route(ReviewMode::Off, &[]).unwrap();
    item.start_render().unwrap();
    item.mark_verified().unwrap();
    PublishPermit::issue(&item, &[]).unwrap()
}

#[test]
fn photos_on_fat32_are_scanned_and_published() {
    let fat = std::env::var_os("CLOAKFRAME_FAT32_DIR")
        .map(PathBuf::from)
        .expect("CLOAKFRAME_FAT32_DIR is not set (see CONTRIBUTING.md)");
    let root = fat.join(format!("cloakframe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let input = root.join("DCIM");
    std::fs::create_dir_all(&input).unwrap();
    std::fs::write(input.join("IMG_0001.JPG"), b"one").unwrap();
    std::fs::write(input.join("IMG_0002.JPG"), b"two").unwrap();

    let scanned = scan(
        &ScanRequest {
            inputs: vec![input.clone()],
            recursive: true,
            include_videos: false,
            output: None,
        },
        &CancelToken::new(),
    )
    .unwrap();
    assert!(scanned.issues.is_empty(), "{:?}", scanned.issues);
    assert_eq!(scanned.files.len(), 2);
    let one = identity(&input.join("IMG_0001.JPG")).unwrap();
    assert_ne!(one, identity(&input.join("IMG_0002.JPG")).unwrap());
    assert_eq!(one, identity(&input.join("img_0001.jpg")).unwrap());

    let destination = root.join("out").join("IMG_0001.JPG");
    let mut out = TempOutput::create(&destination).unwrap();
    out.file().write_all(b"masked").unwrap();
    out.publish(&permit()).unwrap();
    let mut again = TempOutput::create(&destination).unwrap();
    again.file().write_all(b"other").unwrap();
    assert_eq!(
        again.publish(&permit()).unwrap_err().kind(),
        std::io::ErrorKind::AlreadyExists
    );
    assert_eq!(std::fs::read(&destination).unwrap(), b"masked");
    std::fs::remove_dir_all(root).unwrap();
}
