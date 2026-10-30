//! Loading the NVIDIA libraries the CUDA execution provider needs from a folder of the app's
//! own. The provider names them by soname and has no RUNPATH, and `LD_LIBRARY_PATH` is read
//! only when the process starts, so each is opened here by its full path first: the dynamic
//! linker then resolves the provider's dependencies to the copies already loaded.

use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// The NVIDIA driver's library, which the system provides.
const DRIVER: &CStr = c"libcuda.so.1";

/// Opens the driver, then every library in `dir` whose name is in `names`, retrying those whose
/// own dependencies are not loaded yet. The libraries stay loaded for the process. Without the
/// driver the pack is of no use, and this says so before ONNX Runtime is loaded.
pub fn preload(dir: &Path, names: &[&str]) -> Result<(), String> {
    open(DRIVER).map_err(|e| format!("the NVIDIA driver is not installed: {e}"))?;
    let mut pending: Vec<PathBuf> = names.iter().map(|n| dir.join(n)).collect();
    loop {
        let before = pending.len();
        let mut errors = Vec::new();
        pending.retain(|path| {
            let opened = CString::new(path.as_os_str().as_bytes())
                .map_err(|_| format!("{}: not a path", path.display()))
                .and_then(|c| open(&c));
            match opened {
                Ok(()) => false,
                Err(e) => {
                    errors.push(e);
                    true
                }
            }
        });
        if pending.is_empty() {
            return Ok(());
        }
        if pending.len() == before {
            return Err(errors.join("; "));
        }
    }
}

/// Opens `library` for the rest of the process, its symbols visible to those opened later.
#[allow(unsafe_code)]
fn open(library: &CStr) -> Result<(), String> {
    // SAFETY: a NUL-terminated name; the handle is never closed.
    let handle = unsafe { libc::dlopen(library.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) };
    if !handle.is_null() {
        return Ok(());
    }
    // SAFETY: dlerror describes the failed call above on this thread.
    let message = unsafe { libc::dlerror() };
    Err(if message.is_null() {
        format!("{library:?} did not load")
    } else {
        // SAFETY: dlerror's text is NUL-terminated and valid until the next dl call.
        unsafe { CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned()
    })
}
