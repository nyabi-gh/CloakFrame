//! The model files the engine runs: what `manifests/native.toml` pins for each, the folder they
//! are kept in, and how a file gets there (plan 4.6).

pub mod download;

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use cloakframe_core::detection::ModelId;
use cloakframe_core::review::Digest;
use sha2::{Digest as _, Sha256};

use crate::cancel::CancelToken;
use crate::detect::{plate, yolo5face, yunet};
use crate::publish;

/// What a user agrees to by using a model, shown before it is downloaded (plan 4.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Terms {
    /// The license as the manifest records it.
    pub license: &'static str,
    /// The project the model and its terms come from.
    pub source: &'static str,
    pub non_commercial_only: bool,
    /// The terms in a sentence, in English until the app's translations (M6).
    pub summary: &'static str,
}

/// A model file as `manifests/native.toml` pins it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFile {
    pub model: ModelId,
    /// The published name, which the file keeps in the model folder.
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
    pub terms: Terms,
}

impl ModelFile {
    pub fn digest(&self) -> Digest {
        self.sha256.parse().expect("a valid SHA-256")
    }
}

pub static MODEL_FILES: [ModelFile; 3] = [
    ModelFile {
        model: ModelId::Yolo5FaceN,
        name: yolo5face::FILE_NAME,
        url: "https://github.com/yakhyo/yolov5-face-onnx-inference/releases/download/weights/yolov5n_face.onnx",
        sha256: yolo5face::MODEL_SHA256,
        size: 11_021_477,
        terms: Terms {
            license: "non-commercial (upstream terms)",
            source: "https://github.com/deepcam-cn/yolov5-face",
            non_commercial_only: true,
            summary: "Converted from YOLO5Face (GPL-3.0) and trained on WIDER FACE, a dataset for \
                      non-commercial research: use it for non-commercial purposes only.",
        },
    },
    ModelFile {
        model: ModelId::YuNet,
        name: yunet::FILE_NAME,
        url: "https://github.com/opencv/opencv_zoo/raw/47534e27c9851bb1128ccc0102f1145e27f23f98/models/face_detection_yunet/face_detection_yunet_2023mar.onnx",
        sha256: yunet::MODEL_SHA256,
        size: 232_589,
        terms: Terms {
            license: "MIT",
            source: "https://github.com/opencv/opencv_zoo/tree/47534e27c9851bb1128ccc0102f1145e27f23f98/models/face_detection_yunet",
            non_commercial_only: false,
            summary: "From the OpenCV Zoo under the MIT license.",
        },
    },
    ModelFile {
        model: ModelId::PlateYolov9T,
        name: plate::FILE_NAME,
        url: "https://github.com/ankandrew/open-image-models/releases/download/assets/yolo-v9-t-512-license-plates-end2end.onnx",
        sha256: plate::MODEL_SHA256,
        size: 7_799_480,
        terms: Terms {
            license: "MIT (open-image-models; YOLOv9 upstream terms apply)",
            source: "https://github.com/ankandrew/open-image-models",
            non_commercial_only: false,
            summary: "From the open-image-models project under the MIT license; the terms of \
                      YOLOv9, which it builds on, also apply.",
        },
    },
];

pub fn model_file(model: ModelId) -> &'static ModelFile {
    match model {
        ModelId::Yolo5FaceN => &MODEL_FILES[0],
        ModelId::YuNet => &MODEL_FILES[1],
        ModelId::PlateYolov9T => &MODEL_FILES[2],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Present,
    Missing,
    /// A file with the model's name that is not the pinned one.
    Different,
}

#[derive(Debug)]
pub enum InstallError {
    Cancelled,
    /// Reading the source failed: the connection, or the file being copied.
    Source(io::Error),
    TooLarge,
    Truncated {
        received: u64,
    },
    /// The bytes are not the pinned file's.
    Different,
    /// The model folder could not take the file.
    Write(io::Error),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("cancelled"),
            Self::Source(e) => write!(f, "reading it failed: {e}"),
            Self::TooLarge => f.write_str("it is larger than the pinned file"),
            Self::Truncated { received } => write!(f, "it ended after {received} bytes"),
            Self::Different => f.write_str("it is not the pinned file (SHA-256 differs)"),
            Self::Write(e) => write!(f, "the model folder cannot take it: {e}"),
        }
    }
}

impl std::error::Error for InstallError {}

/// A folder of model files under their published names. Nothing checks who can write to it
/// (D5): a session is made only from bytes with the pinned hash, so a changed file is refused
/// when it is loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelStore {
    dir: PathBuf,
}

impl ModelStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path(&self, model: ModelId) -> PathBuf {
        self.dir.join(model_file(model).name)
    }

    /// The model's bytes as they are. The detector checks them against the pinned hash before it
    /// makes a session from them, so the file is read once.
    pub fn read(&self, model: ModelId) -> io::Result<Vec<u8>> {
        fs::read(self.path(model))
    }

    pub fn status(&self, model: ModelId) -> io::Result<Status> {
        match fs::read(self.path(model)) {
            Ok(bytes) if Digest(Sha256::digest(&bytes).into()) == model_file(model).digest() => {
                Ok(Status::Present)
            }
            Ok(_) => Ok(Status::Different),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Status::Missing),
            Err(e) => Err(e),
        }
    }

    /// Writes `source` to a temporary file in the folder, hashing it on the way, and gives it the
    /// model's name only if it is the pinned file. A file already under that name is replaced.
    /// The folder is made with default permissions. `progress` gets the bytes written so far.
    pub fn install(
        &self,
        model: ModelId,
        source: &mut dyn Read,
        cancel: &CancelToken,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), InstallError> {
        self.install_file(model_file(model), source, cancel, progress)
    }

    fn install_file(
        &self,
        pinned: &ModelFile,
        source: &mut dyn Read,
        cancel: &CancelToken,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), InstallError> {
        fs::create_dir_all(&self.dir).map_err(InstallError::Write)?;
        // Leftovers of a download that was killed. Failing to remove them does not affect this one.
        let _ = publish::remove_stale_temp_files(&self.dir, SystemTime::now());
        let (file, path) =
            publish::create_temp(&self.dir, Some("onnx".as_ref())).map_err(InstallError::Write)?;
        let mut temp = Temp {
            file: Some(file),
            path,
            keep: false,
        };
        let file = temp.file.as_mut().expect("open until renamed");

        let mut hasher = Sha256::new();
        let mut received = 0u64;
        let mut buffer = vec![0; 64 * 1024];
        loop {
            if cancel.is_cancelled() {
                return Err(InstallError::Cancelled);
            }
            let n = match source.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(InstallError::Source(e)),
            };
            received += n as u64;
            if received > pinned.size {
                return Err(InstallError::TooLarge);
            }
            hasher.update(&buffer[..n]);
            file.write_all(&buffer[..n]).map_err(InstallError::Write)?;
            progress(received);
        }
        if received < pinned.size {
            return Err(InstallError::Truncated { received });
        }
        if Digest(hasher.finalize().into()) != pinned.digest() {
            return Err(InstallError::Different);
        }
        file.sync_all().map_err(InstallError::Write)?;
        temp.file.take();
        fs::rename(&temp.path, self.dir.join(pinned.name)).map_err(InstallError::Write)?;
        temp.keep = true;
        // The file is in place and checked. Should a crash undo the rename, the model is simply
        // missing and downloaded again.
        let _ = publish::sync_folder(&self.dir);
        Ok(())
    }

    /// Copies `model` from the first of `folders` that holds the pinned file, when this folder
    /// does not have the model. Returns where it came from. A file there that is not the pinned
    /// one, or cannot be read, is passed over; only failing to write here is an error.
    pub fn import(
        &self,
        model: ModelId,
        folders: &[PathBuf],
    ) -> Result<Option<PathBuf>, InstallError> {
        self.import_file(model_file(model), folders)
    }

    fn import_file(
        &self,
        pinned: &ModelFile,
        folders: &[PathBuf],
    ) -> Result<Option<PathBuf>, InstallError> {
        let destination = self.dir.join(pinned.name);
        if destination.try_exists().map_err(InstallError::Write)? {
            return Ok(None);
        }
        for folder in folders {
            let source = folder.join(pinned.name);
            let Ok(mut file) = File::open(&source) else {
                continue;
            };
            match self.install_file(pinned, &mut file, &CancelToken::new(), &mut |_| {}) {
                Ok(()) => return Ok(Some(source)),
                Err(e @ InstallError::Write(_)) => return Err(e),
                Err(_) => {}
            }
        }
        Ok(None)
    }
}

/// A temporary file, closed and then removed unless it got the model's name. Closed first: on
/// Windows the name of a file deleted while open can stay until its last handle is closed.
struct Temp {
    file: Option<File>,
    path: PathBuf,
    keep: bool,
}

impl Drop for Temp {
    fn drop(&mut self) {
        self.file.take();
        if !self.keep {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;

    fn manifest_value<'a>(entry: &'a str, key: &str) -> &'a str {
        // `lines` also strips the CR of a Windows checkout.
        entry
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{key} = ")))
            .unwrap_or_else(|| panic!("no {key} in {entry}"))
            .trim_matches('"')
    }

    #[test]
    fn the_pinned_files_are_the_manifests() {
        let manifest = include_str!("../../../../manifests/native.toml");
        for file in &MODEL_FILES {
            assert_eq!(model_file(file.model), file);
            let entry = manifest
                .split("[[artifact]]")
                .find(|e| e.contains(&format!("sha256 = \"{}\"", file.sha256)))
                .unwrap_or_else(|| panic!("{:?} is not in the manifest", file.model));
            assert_eq!(manifest_value(entry, "url"), file.url);
            assert!(file.url.ends_with(&format!("/{}", file.name)), "{file:?}");
            assert_eq!(manifest_value(entry, "size"), file.size.to_string());
            assert_eq!(manifest_value(entry, "license"), file.terms.license);
            assert_eq!(manifest_value(entry, "source"), file.terms.source);
            assert_eq!(manifest_value(entry, "use"), "download");
            assert_eq!(
                file.terms.non_commercial_only,
                file.terms.license.contains("non-commercial")
            );
        }
    }

    const NAME: &str = "test-model.onnx";

    /// A model file pinned to `bytes`, standing in for the real ones so the tests need no assets.
    fn pin(bytes: &[u8]) -> ModelFile {
        let hex: String = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        ModelFile {
            name: NAME,
            sha256: Box::leak(hex.into_boxed_str()),
            size: bytes.len() as u64,
            ..MODEL_FILES[1]
        }
    }

    fn model_bytes() -> Vec<u8> {
        (0..200_000u32).map(|i| (i * 7 % 251) as u8).collect()
    }

    fn store(dir: &TestDir) -> ModelStore {
        ModelStore::new(dir.path().join("models"))
    }

    fn files_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn install(store: &ModelStore, pinned: &ModelFile, bytes: &[u8]) -> Result<(), InstallError> {
        store.install_file(pinned, &mut &bytes[..], &CancelToken::new(), &mut |_| {})
    }

    #[test]
    fn the_pinned_file_is_installed_under_its_name() {
        let dir = TestDir::new();
        let store = store(&dir);
        let bytes = model_bytes();
        let pinned = pin(&bytes);
        let mut seen = Vec::new();
        store
            .install_file(&pinned, &mut &bytes[..], &CancelToken::new(), &mut |n| {
                seen.push(n)
            })
            .unwrap();
        assert_eq!(seen.last(), Some(&(bytes.len() as u64)));
        assert!(seen.len() > 1);
        assert_eq!(files_in(store.dir()), [NAME]);
        assert_eq!(fs::read(store.dir().join(NAME)).unwrap(), bytes);
    }

    #[test]
    fn anything_but_the_pinned_file_leaves_nothing_behind() {
        let dir = TestDir::new();
        let store = store(&dir);
        let bytes = model_bytes();
        let pinned = pin(&bytes);
        let mut changed = bytes.clone();
        changed[1000] ^= 1;
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(matches!(
            install(&store, &pinned, &changed),
            Err(InstallError::Different)
        ));
        assert!(matches!(
            install(&store, &pinned, &longer),
            Err(InstallError::TooLarge)
        ));
        assert!(matches!(
            install(&store, &pinned, &bytes[..1000]),
            Err(InstallError::Truncated { received: 1000 })
        ));
        let cancelled = CancelToken::new();
        cancelled.cancel();
        assert!(matches!(
            store.install_file(&pinned, &mut &bytes[..], &cancelled, &mut |_| {}),
            Err(InstallError::Cancelled)
        ));
        let mut broken = BrokenAfter(&bytes[..5000]);
        assert!(matches!(
            store.install_file(&pinned, &mut broken, &CancelToken::new(), &mut |_| {}),
            Err(InstallError::Source(_))
        ));
        assert_eq!(files_in(store.dir()), Vec::<String>::new());
    }

    /// Gives its bytes, then fails as a dropped connection does.
    struct BrokenAfter<'a>(&'a [u8]);

    impl Read for BrokenAfter<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.0.is_empty() {
                return Err(io::Error::new(io::ErrorKind::ConnectionReset, "reset"));
            }
            self.0.read(buf)
        }
    }

    #[test]
    fn the_file_is_named_only_once_it_is_checked() {
        let dir = TestDir::new();
        let store = store(&dir);
        let bytes = model_bytes();
        let folder = store.dir().to_path_buf();
        let mut seen = Vec::new();
        let mut reader = Watching {
            bytes: &bytes,
            folder: &folder,
            seen: &mut seen,
        };
        store
            .install_file(&pin(&bytes), &mut reader, &CancelToken::new(), &mut |_| {})
            .unwrap();
        assert!(seen.len() > 1);
        for names in seen {
            assert_eq!(names.len(), 1);
            assert!(names[0].starts_with(publish::TEMP_PREFIX), "{names:?}");
        }
        assert_eq!(files_in(&folder), [NAME]);
    }

    /// Records the folder's names at every read.
    struct Watching<'a> {
        bytes: &'a [u8],
        folder: &'a Path,
        seen: &'a mut Vec<Vec<String>>,
    }

    impl Read for Watching<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.seen.push(files_in(self.folder));
            let n = buf.len().min(self.bytes.len()).min(4096);
            buf[..n].copy_from_slice(&self.bytes[..n]);
            self.bytes = &self.bytes[n..];
            Ok(n)
        }
    }

    #[test]
    fn a_changed_file_is_replaced_and_old_temp_files_go() {
        let dir = TestDir::new();
        let store = store(&dir);
        let bytes = model_bytes();
        let pinned = pin(&bytes);
        fs::create_dir_all(store.dir()).unwrap();
        fs::write(store.dir().join(NAME), b"not the model").unwrap();
        let stale = store
            .dir()
            .join(format!("{}old.onnx", publish::TEMP_PREFIX));
        fs::write(&stale, b"half a download").unwrap();
        File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(SystemTime::now() - publish::STALE_AFTER * 2)
            .unwrap();

        install(&store, &pinned, &bytes).unwrap();
        assert_eq!(files_in(store.dir()), [NAME]);
        assert_eq!(fs::read(store.dir().join(NAME)).unwrap(), bytes);
    }

    #[test]
    fn the_status_says_whether_the_file_is_the_pinned_one() {
        let dir = TestDir::new();
        let store = store(&dir);
        assert_eq!(store.status(ModelId::YuNet).unwrap(), Status::Missing);
        fs::create_dir_all(store.dir()).unwrap();
        fs::write(store.path(ModelId::YuNet), b"not the model").unwrap();
        assert_eq!(store.status(ModelId::YuNet).unwrap(), Status::Different);
        assert_eq!(
            store.path(ModelId::YuNet),
            store.dir().join("face_detection_yunet_2023mar.onnx")
        );
    }

    #[test]
    fn a_model_is_copied_from_the_first_folder_that_has_the_pinned_file() {
        let dir = TestDir::new();
        let store = store(&dir);
        let bytes = model_bytes();
        let pinned = pin(&bytes);
        dir.write_bytes(&format!("changed/{NAME}"), b"older");
        let good = dir.write_bytes(&format!("good/{NAME}"), &bytes);
        dir.write_bytes(&format!("also/{NAME}"), &bytes);
        let folders: Vec<PathBuf> = ["missing", "changed", "good", "also"]
            .iter()
            .map(|f| dir.path().join(f))
            .collect();

        assert_eq!(store.import_file(&pinned, &folders[..2]).unwrap(), None);
        assert!(!store.dir().join(NAME).exists());
        assert_eq!(
            store.import_file(&pinned, &folders).unwrap(),
            Some(good.clone())
        );
        assert_eq!(fs::read(store.dir().join(NAME)).unwrap(), bytes);
        // The source stays, and a model the folder has is not copied again.
        assert_eq!(fs::read(&good).unwrap(), bytes);
        fs::remove_file(&good).unwrap();
        assert_eq!(store.import_file(&pinned, &folders).unwrap(), None);
        // A folder that is the store itself has nothing to give.
        fs::remove_file(store.dir().join(NAME)).unwrap();
        let own = vec![store.dir().to_path_buf()];
        assert_eq!(store.import_file(&pinned, &own).unwrap(), None);
    }
}
