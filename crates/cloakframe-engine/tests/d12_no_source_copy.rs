//! D12: publishing never writes to the system temp folder; the only extra file is the hidden
//! temp output next to the destination. One test per binary, because it changes the environment.

mod common;

use std::io::Write;

use cloakframe_engine::publish::{TEMP_PREFIX, TempOutput};
use common::permit;

#[test]
#[allow(unsafe_code)]
fn the_system_temp_folder_stays_untouched() {
    let root = std::env::temp_dir().join(format!("cloakframe-d12-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let private_temp = root.join("tmp");
    std::fs::create_dir_all(&private_temp).unwrap();
    // SAFETY: this binary has a single test and sets the variables before starting any thread.
    unsafe {
        for name in ["TMPDIR", "TMP", "TEMP"] {
            std::env::set_var(name, &private_temp);
        }
    }
    assert_eq!(std::env::temp_dir(), private_temp);

    let source = root.join("input/photo.png");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, vec![7u8; 1 << 20]).unwrap();
    let bytes = std::fs::read(&source).unwrap();

    let out_dir = root.join("out");
    let mut out = TempOutput::create(&out_dir.join("photo.png")).unwrap();
    out.file().write_all(&bytes).unwrap();
    let during: Vec<String> = std::fs::read_dir(&out_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(during.len(), 1);
    assert!(during[0].starts_with(TEMP_PREFIX));

    out.publish(&permit()).unwrap();

    assert_eq!(std::fs::read_dir(&private_temp).unwrap().count(), 0);
    let after: Vec<String> = std::fs::read_dir(&out_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(after, ["photo.png"]);
    std::fs::remove_dir_all(root).unwrap();
}
