//! Where the app finds ONNX Runtime and FFmpeg: next to its executable, where its package puts
//! them (M7). Development builds also take the paths the tests use, from the environment.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub ort_library: Option<PathBuf>,
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
}

const ORT_LIBRARY: &str = if cfg!(target_os = "macos") {
    "libonnxruntime.dylib"
} else if cfg!(windows) {
    "onnxruntime.dll"
} else {
    "libonnxruntime.so"
};

fn executable(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

impl Tools {
    pub fn locate() -> Self {
        let beside = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf));
        let found = |name: &str, variable: &str| {
            let from_environment = cfg!(debug_assertions)
                .then(|| std::env::var_os(variable).map(PathBuf::from))
                .flatten();
            from_environment
                .or_else(|| beside.as_ref().map(|dir| dir.join(name)))
                .filter(|path| path.is_file())
        };
        Self {
            ort_library: found(ORT_LIBRARY, "CLOAKFRAME_ORT_LIBRARY"),
            ffmpeg: found(&executable("ffmpeg"), "CLOAKFRAME_FFMPEG"),
            ffprobe: found(&executable("ffprobe"), "CLOAKFRAME_FFPROBE"),
        }
    }

    pub fn videos(&self) -> bool {
        self.ffmpeg.is_some() && self.ffprobe.is_some()
    }
}
