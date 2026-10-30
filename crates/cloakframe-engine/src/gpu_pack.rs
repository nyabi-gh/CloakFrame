//! The NVIDIA GPU pack for Linux x86_64 (docs/M7_ACCELERATORS.md): ONNX Runtime's CUDA 13 build
//! and the NVIDIA libraries its CUDA provider needs, about 1.3 GB, downloaded only when the user
//! turns it on and accepts NVIDIA's terms. Each archive is pinned by URL, size and SHA-256, and
//! only the named files are taken out of it, each of a pinned size. The pack is made in a staging
//! folder and renamed into place once every file is there, so a pack folder is always complete.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::cancel::CancelToken;
use crate::model_files::download::{Fetch, FetchError};

/// The pack's folder name: a new pack is a new folder, so an older one is never half replaced.
pub const ID: &str = "cuda13-ort1.30.0";

/// ONNX Runtime's library in the pack, loaded in place of the CPU build the app carries.
pub const ORT_LIBRARY: &str = "libonnxruntime.so.1.30.0";

/// Where NVIDIA's terms are published; the user accepts both before the download.
pub const TERMS: [(&str, &str); 2] = [
    (
        "NVIDIA CUDA Toolkit EULA",
        "https://docs.nvidia.com/cuda/eula/index.html",
    ),
    (
        "NVIDIA cuDNN Software License Agreement",
        "https://docs.nvidia.com/deeplearning/cudnn/latest/reference/eula.html",
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    TarGz,
    Zip,
}

/// One file taken out of an archive.
#[derive(Debug, Clone, Copy)]
pub struct Member {
    /// Its path in the archive.
    pub path: &'static str,
    /// Its name in the pack.
    pub name: &'static str,
    pub size: u64,
    /// The CUDA provider's dependencies, opened by the app before ONNX Runtime
    /// (`detect::cuda::preload`).
    pub preload: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Archive {
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub format: Format,
    pub members: &'static [Member],
}

pub const fn library(path: &'static str, name: &'static str, size: u64) -> Member {
    Member {
        path,
        name,
        size,
        preload: true,
    }
}

pub const fn file(path: &'static str, name: &'static str, size: u64) -> Member {
    Member {
        path,
        name,
        size,
        preload: false,
    }
}

pub const ARCHIVES: [Archive; 5] = [
    Archive {
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-linux-x64-gpu_cuda13-1.30.0.tgz",
        size: 235_697_517,
        sha256: "382d79133112388cf94ce5855789b7c9bef12bef76a08b6b277e5a317213adcd",
        format: Format::TarGz,
        members: &[
            file(
                "onnxruntime-linux-x64-gpu_cuda13-1.30.0/lib/libonnxruntime.so.1.30.0",
                ORT_LIBRARY,
                32_515_904,
            ),
            file(
                "onnxruntime-linux-x64-gpu_cuda13-1.30.0/lib/libonnxruntime_providers_shared.so",
                "libonnxruntime_providers_shared.so",
                14_632,
            ),
            file(
                "onnxruntime-linux-x64-gpu_cuda13-1.30.0/lib/libonnxruntime_providers_cuda.so",
                "libonnxruntime_providers_cuda.so",
                272_054_000,
            ),
            file(
                "onnxruntime-linux-x64-gpu_cuda13-1.30.0/LICENSE",
                "ONNXRUNTIME_LICENSE.txt",
                1_073,
            ),
            file(
                "onnxruntime-linux-x64-gpu_cuda13-1.30.0/ThirdPartyNotices.txt",
                "ONNXRUNTIME_THIRD_PARTY_NOTICES.txt",
                338_088,
            ),
        ],
    },
    Archive {
        url: "https://files.pythonhosted.org/packages/98/8a/3431271f6344874b8f1ac03f16b3d679c91493f8da63f716160403e6d0a0/nvidia_cuda_runtime-13.4.92-py3-none-manylinux2014_x86_64.manylinux_2_17_x86_64.whl",
        size: 2_494_438,
        sha256: "9641f797da20ce1dd8e779b6e96d08cf9ba564cec8e8225458811ee26423f3a5",
        format: Format::Zip,
        members: &[
            library(
                "nvidia/cu13/lib/libcudart.so.13",
                "libcudart.so.13",
                798_496,
            ),
            file(
                "nvidia_cuda_runtime-13.4.92.dist-info/licenses/License.txt",
                "NVIDIA_CUDA_EULA.txt",
                59_262,
            ),
        ],
    },
    Archive {
        url: "https://files.pythonhosted.org/packages/04/36/c6f716dbd080fa030e6056261f238fb82c2b78d94e5074460740f61384b3/nvidia_cublas-13.8.2.12-py3-none-manylinux_2_27_x86_64.whl",
        size: 439_349_182,
        sha256: "21721774d8e7044debec3034ecc1a8cd22e173eb6dae5d7f5f8a014852382894",
        format: Format::Zip,
        members: &[
            library(
                "nvidia/cu13/lib/libcublasLt.so.13",
                "libcublasLt.so.13",
                545_561_016,
            ),
            library(
                "nvidia/cu13/lib/libcublas.so.13",
                "libcublas.so.13",
                57_594_520,
            ),
        ],
    },
    Archive {
        url: "https://files.pythonhosted.org/packages/07/73/3ee8e5b4cb891401e603ffd3a59b35c6afe785fd2de123afe7c7029603dc/nvidia_curand-10.4.4.72-py3-none-manylinux_2_27_x86_64.whl",
        size: 61_498_332,
        sha256: "25c3457ae7a224fdd484dab90b0fc5dc0e842fab5db3012afa4a5bd2af4eb7e5",
        format: Format::Zip,
        members: &[library(
            "nvidia/cu13/lib/libcurand.so.10",
            "libcurand.so.10",
            126_468_312,
        )],
    },
    // All of cuDNN: without `engines_tensor_ir` and `engines_runtime_compiled` the provider still
    // starts but runs the convolutions on the CPU or fails on the first image.
    Archive {
        url: "https://files.pythonhosted.org/packages/af/75/96ea5c5368eb595c39d629cde08a66227a864e71ccb0e593add2612bb952/nvidia_cudnn_cu13-9.27.0.42-py3-none-manylinux_2_27_x86_64.whl",
        size: 536_771_498,
        sha256: "9677e76f21862eb5da7ee5ed69d544738b2d8b5c3ce7e5ec125c5592e6cdbdc8",
        format: Format::Zip,
        members: &[
            library("nvidia/cudnn/lib/libcudnn.so.9", "libcudnn.so.9", 133_336),
            library(
                "nvidia/cudnn/lib/libcudnn_graph.so.9",
                "libcudnn_graph.so.9",
                134_437_112,
            ),
            library(
                "nvidia/cudnn/lib/libcudnn_ops.so.9",
                "libcudnn_ops.so.9",
                39_855_912,
            ),
            library(
                "nvidia/cudnn/lib/libcudnn_cnn.so.9",
                "libcudnn_cnn.so.9",
                4_203_992,
            ),
            library(
                "nvidia/cudnn/lib/libcudnn_adv.so.9",
                "libcudnn_adv.so.9",
                109_169_128,
            ),
            library(
                "nvidia/cudnn/lib/libcudnn_ext.so.9",
                "libcudnn_ext.so.9",
                8_402_008,
            ),
            library(
                "nvidia/cudnn/lib/libcudnn_heuristic.so.9",
                "libcudnn_heuristic.so.9",
                96_473_904,
            ),
            library(
                "nvidia/cudnn/lib/libcudnn_engines_precompiled.so.9",
                "libcudnn_engines_precompiled.so.9",
                272_654_912,
            ),
            library(
                "nvidia/cudnn/lib/libcudnn_engines_runtime_compiled.so.9",
                "libcudnn_engines_runtime_compiled.so.9",
                44_075_680,
            ),
            library(
                "nvidia/cudnn/lib/libcudnn_engines_tensor_ir.so.9",
                "libcudnn_engines_tensor_ir.so.9",
                216_026_256,
            ),
            file(
                "nvidia_cudnn_cu13-9.27.0.42.dist-info/licenses/License.txt",
                "NVIDIA_CUDNN_LICENSE.txt",
                18_174,
            ),
        ],
    },
];

/// What the download takes, in bytes.
pub fn download_size() -> u64 {
    ARCHIVES.iter().map(|a| a.size).sum()
}

/// What the pack takes on disk, in bytes.
pub fn installed_size() -> u64 {
    members(&ARCHIVES).map(|m| m.size).sum()
}

fn members(archives: &'static [Archive]) -> impl Iterator<Item = &'static Member> {
    archives.iter().flat_map(|a| a.members.iter())
}

/// The libraries to open before ONNX Runtime, by their names in the pack.
pub fn preloaded() -> Vec<&'static str> {
    members(&ARCHIVES)
        .filter(|m| m.preload)
        .map(|m| m.name)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Missing,
    Ready,
    /// The folder is there but a file is missing or of another size.
    Damaged(String),
}

#[derive(Debug)]
pub enum PackError {
    Fetch(FetchError),
    Source(io::Error),
    Write(io::Error),
    TooLarge(&'static str),
    Truncated {
        url: &'static str,
        received: u64,
    },
    Different(&'static str),
    /// The archive lacks a pinned file, or has it at another size.
    Content(String),
    Cancelled,
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fetch(e) => e.fmt(f),
            Self::Source(e) => write!(f, "the download broke off: {e}"),
            Self::Write(e) => write!(f, "cannot write the GPU pack: {e}"),
            Self::TooLarge(url) => write!(f, "{url} is larger than pinned"),
            Self::Truncated { url, received } => {
                write!(f, "{url} ended after {received} bytes")
            }
            Self::Different(url) => write!(f, "{url} is not the pinned file"),
            Self::Content(reason) => f.write_str(reason),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for PackError {}

/// The GPU pack under `root`, the app's `gpu` folder.
#[derive(Debug, Clone)]
pub struct GpuPack {
    root: PathBuf,
    archives: &'static [Archive],
}

impl GpuPack {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            archives: &ARCHIVES,
        }
    }

    /// What its archives take to download, in bytes.
    pub fn download_size(&self) -> u64 {
        self.archives.iter().map(|a| a.size).sum()
    }

    /// The pack's own folder.
    pub fn dir(&self) -> PathBuf {
        self.root.join(ID)
    }

    pub fn status(&self) -> Status {
        let dir = self.dir();
        if !dir.exists() {
            return Status::Missing;
        }
        for member in members(self.archives) {
            let path = dir.join(member.name);
            match fs::metadata(&path) {
                Ok(m) if m.len() == member.size => {}
                Ok(m) => {
                    return Status::Damaged(format!(
                        "{} is {} bytes, not {}",
                        path.display(),
                        m.len(),
                        member.size
                    ));
                }
                Err(e) => return Status::Damaged(format!("{}: {e}", path.display())),
            }
        }
        Status::Ready
    }

    /// Downloads and unpacks every archive, replacing a damaged pack. `progress` gets the bytes
    /// received so far of [`download_size`].
    pub fn install(
        &self,
        fetch: &dyn Fetch,
        cancel: &CancelToken,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), PackError> {
        fs::create_dir_all(&self.root).map_err(PackError::Write)?;
        let staging = Leftover(self.root.join(format!(".staging-{ID}")));
        let download = Leftover(self.root.join(format!(".download-{ID}")));
        staging.clear().map_err(PackError::Write)?;
        download.clear().map_err(PackError::Write)?;
        fs::create_dir(&staging.0).map_err(PackError::Write)?;
        let mut done = 0;
        for archive in self.archives {
            fetch_archive(archive, fetch, &download.0, cancel, &mut |n| {
                progress(done + n)
            })?;
            done += archive.size;
            unpack(archive, &download.0, &staging.0, cancel)?;
            fs::remove_file(&download.0).map_err(PackError::Write)?;
        }
        let dir = self.dir();
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(PackError::Write)?;
        }
        fs::rename(&staging.0, &dir).map_err(PackError::Write)?;
        Ok(())
    }

    /// Removes the pack, and leftovers of an install that did not finish.
    pub fn remove(&self) -> io::Result<()> {
        for path in [
            self.dir(),
            self.root.join(format!(".staging-{ID}")),
            self.root.join(format!(".download-{ID}")),
        ] {
            Leftover(path).clear()?;
        }
        Ok(())
    }
}

/// A file or folder removed when an install stops early.
struct Leftover(PathBuf);

impl Leftover {
    fn clear(&self) -> io::Result<()> {
        match fs::symlink_metadata(&self.0) {
            Ok(m) if m.is_dir() => fs::remove_dir_all(&self.0),
            Ok(_) => fs::remove_file(&self.0),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

impl Drop for Leftover {
    fn drop(&mut self) {
        let _ = self.clear();
    }
}

fn fetch_archive(
    archive: &Archive,
    fetch: &dyn Fetch,
    to: &Path,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(u64),
) -> Result<(), PackError> {
    let mut body = fetch.get(archive.url).map_err(PackError::Fetch)?;
    if body.length.is_some_and(|n| n > archive.size) {
        return Err(PackError::TooLarge(archive.url));
    }
    let mut file = File::create(to).map_err(PackError::Write)?;
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    let mut buffer = vec![0; 1 << 20];
    loop {
        if cancel.is_cancelled() {
            return Err(PackError::Cancelled);
        }
        let n = match body.reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(PackError::Source(e)),
        };
        received += n as u64;
        if received > archive.size {
            return Err(PackError::TooLarge(archive.url));
        }
        hasher.update(&buffer[..n]);
        file.write_all(&buffer[..n]).map_err(PackError::Write)?;
        progress(received);
    }
    if received < archive.size {
        return Err(PackError::Truncated {
            url: archive.url,
            received,
        });
    }
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if digest != archive.sha256 {
        return Err(PackError::Different(archive.url));
    }
    file.sync_all().map_err(PackError::Write)
}

/// Copies the archive's pinned members into `to`, each checked for its size.
fn unpack(
    archive: &Archive,
    from: &Path,
    to: &Path,
    cancel: &CancelToken,
) -> Result<(), PackError> {
    let mut found = vec![false; archive.members.len()];
    let mut take = |path: &str, reader: &mut dyn Read| -> Result<(), PackError> {
        let Some(i) = archive.members.iter().position(|m| m.path == path) else {
            return Ok(());
        };
        let member = &archive.members[i];
        let mut out = File::create(to.join(member.name)).map_err(PackError::Write)?;
        let copied =
            io::copy(&mut reader.take(member.size + 1), &mut out).map_err(PackError::Source)?;
        if copied != member.size {
            return Err(PackError::Content(format!(
                "{path} in {} is {copied} bytes, not {}",
                archive.url, member.size
            )));
        }
        out.sync_all().map_err(PackError::Write)?;
        found[i] = true;
        Ok(())
    };
    let file = File::open(from).map_err(PackError::Write)?;
    match archive.format {
        Format::TarGz => {
            let mut entries = tar::Archive::new(flate2::read::GzDecoder::new(file));
            for entry in entries.entries().map_err(PackError::Source)? {
                if cancel.is_cancelled() {
                    return Err(PackError::Cancelled);
                }
                let mut entry = entry.map_err(PackError::Source)?;
                if !entry.header().entry_type().is_file() {
                    continue;
                }
                let path = entry
                    .path()
                    .map_err(PackError::Source)?
                    .to_string_lossy()
                    .into_owned();
                take(&path, &mut entry)?;
            }
        }
        Format::Zip => {
            let mut zip = zip::ZipArchive::new(file)
                .map_err(|e| PackError::Content(format!("{}: {e}", archive.url)))?;
            for member in archive.members {
                if cancel.is_cancelled() {
                    return Err(PackError::Cancelled);
                }
                let mut entry = zip.by_name(member.path).map_err(|e| {
                    PackError::Content(format!("{} in {}: {e}", member.path, archive.url))
                })?;
                take(member.path, &mut entry)?;
            }
        }
    }
    match archive.members.iter().zip(&found).find(|(_, f)| !**f) {
        Some((missing, _)) => Err(PackError::Content(format!(
            "{} has no {}",
            archive.url, missing.path
        ))),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_files::download::Body;
    use crate::test_support::TestDir;
    use std::collections::HashMap;
    use std::io::Cursor;

    /// Serves fixed bytes per URL.
    struct Serves(HashMap<&'static str, Vec<u8>>);

    impl Fetch for Serves {
        fn get(&self, url: &str) -> Result<Body, FetchError> {
            let bytes = self.0.get(url).cloned().ok_or(FetchError::Status(404))?;
            Ok(Body {
                length: Some(bytes.len() as u64),
                reader: Box::new(Cursor::new(bytes)),
            })
        }
    }

    fn tar_gz(files: &[(&str, &[u8])]) -> Vec<u8> {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut builder = tar::Builder::new(gz);
        for (path, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *bytes).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (path, bytes) in files {
            writer.start_file(*path, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn sha256(bytes: &[u8]) -> &'static str {
        let hex: String = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        hex.leak()
    }

    const ORT: [Member; 2] = [
        file("ort/lib/libonnxruntime.so.1.30.0", ORT_LIBRARY, 4),
        file(
            "ort/lib/libonnxruntime_providers_cuda.so",
            "libonnxruntime_providers_cuda.so",
            3,
        ),
    ];
    const CUDNN: [Member; 2] = [
        library("nvidia/cudnn/lib/libcudnn.so.9", "libcudnn.so.9", 2),
        file("dist-info/License.txt", "NVIDIA_CUDNN_LICENSE.txt", 5),
    ];

    /// A pack of a tar.gz and a wheel, and the server that has them.
    fn fake() -> (&'static [Archive], Serves) {
        let tgz = tar_gz(&[
            ("ort/lib/libonnxruntime.so.1.30.0", b"core"),
            ("ort/lib/libonnxruntime_providers_tensorrt.so", b"not taken"),
            ("ort/lib/libonnxruntime_providers_cuda.so", b"gpu"),
        ]);
        let wheel = zip(&[
            ("nvidia/cudnn/lib/libcudnn.so.9", b"dn"),
            ("dist-info/License.txt", b"terms"),
        ]);
        let archives = vec![
            Archive {
                url: "https://example.test/ort.tgz",
                size: tgz.len() as u64,
                sha256: sha256(&tgz),
                format: Format::TarGz,
                members: &ORT,
            },
            Archive {
                url: "https://example.test/cudnn.whl",
                size: wheel.len() as u64,
                sha256: sha256(&wheel),
                format: Format::Zip,
                members: &CUDNN,
            },
        ];
        let served = Serves(HashMap::from([
            ("https://example.test/ort.tgz", tgz),
            ("https://example.test/cudnn.whl", wheel),
        ]));
        (archives.leak(), served)
    }

    fn pack(dir: &TestDir, archives: &'static [Archive]) -> GpuPack {
        GpuPack {
            root: dir.path().join("gpu"),
            archives,
        }
    }

    fn entries(root: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_pinned_files_come_out_of_their_archives() {
        let dir = TestDir::new();
        let (archives, served) = fake();
        let pack = pack(&dir, archives);
        assert_eq!(pack.status(), Status::Missing);
        let mut seen = 0;
        pack.install(&served, &CancelToken::new(), &mut |n| seen = n)
            .unwrap();
        assert_eq!(seen, archives.iter().map(|a| a.size).sum::<u64>());
        assert_eq!(pack.status(), Status::Ready);
        assert_eq!(entries(&dir.path().join("gpu")), [ID]);
        assert_eq!(
            entries(&pack.dir()),
            [
                "NVIDIA_CUDNN_LICENSE.txt",
                "libcudnn.so.9",
                ORT_LIBRARY,
                "libonnxruntime_providers_cuda.so"
            ]
        );
        assert_eq!(fs::read(pack.dir().join(ORT_LIBRARY)).unwrap(), b"core");
        assert_eq!(fs::read(pack.dir().join("libcudnn.so.9")).unwrap(), b"dn");

        fs::write(pack.dir().join("libcudnn.so.9"), b"d").unwrap();
        assert!(matches!(pack.status(), Status::Damaged(_)));
        pack.install(&served, &CancelToken::new(), &mut |_| {})
            .unwrap();
        assert_eq!(pack.status(), Status::Ready);

        pack.remove().unwrap();
        assert_eq!(pack.status(), Status::Missing);
        assert!(entries(&dir.path().join("gpu")).is_empty());
    }

    #[test]
    fn a_failed_install_leaves_nothing_behind() {
        let dir = TestDir::new();
        let (archives, mut served) = fake();
        let pack = pack(&dir, archives);
        served.0.get_mut("https://example.test/cudnn.whl").unwrap()[40] ^= 1;
        assert!(matches!(
            pack.install(&served, &CancelToken::new(), &mut |_| {}),
            Err(PackError::Different("https://example.test/cudnn.whl"))
        ));
        assert_eq!(pack.status(), Status::Missing);
        assert!(entries(&dir.path().join("gpu")).is_empty());

        let (_, served) = fake();
        let cancel = CancelToken::new();
        cancel.cancel();
        assert!(matches!(
            pack.install(&served, &cancel, &mut |_| {}),
            Err(PackError::Cancelled)
        ));
        assert!(entries(&dir.path().join("gpu")).is_empty());
    }

    #[test]
    fn an_archive_without_a_pinned_file_or_of_another_size_is_refused() {
        let dir = TestDir::new();
        let (archives, served) = fake();
        const SHORT: [Member; 1] = [file("ort/lib/libonnxruntime.so.1.30.0", ORT_LIBRARY, 5)];
        const ABSENT: [Member; 1] = [file("ort/lib/libonnxruntime.so", ORT_LIBRARY, 4)];
        for members in [&SHORT[..], &ABSENT[..]] {
            let mut changed = archives.to_vec();
            changed[0].members = members;
            let pack = pack(&dir, changed.leak());
            assert!(matches!(
                pack.install(&served, &CancelToken::new(), &mut |_| {}),
                Err(PackError::Content(_))
            ));
            assert!(entries(&dir.path().join("gpu")).is_empty());
        }
    }

    #[test]
    fn the_real_pack_is_the_one_measured() {
        assert_eq!(download_size(), 1_275_810_967);
        assert_eq!(preloaded().len(), 14);
        let names: Vec<&str> = members(&ARCHIVES).map(|m| m.name).collect();
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len());
        assert!(names.contains(&ORT_LIBRARY));
        let manifest = include_str!("../../../manifests/native.toml");
        for archive in &ARCHIVES {
            let entry = manifest
                .split("[[artifact]]")
                .find(|e| e.contains(&format!("url = \"{}\"", archive.url)))
                .unwrap_or_else(|| panic!("{} is not in manifests/native.toml", archive.url));
            assert!(
                entry.contains(&format!("sha256 = \"{}\"", archive.sha256)),
                "{entry}"
            );
            assert!(
                entry.contains(&format!("size = {}", archive.size)),
                "{entry}"
            );
        }
    }
}
