//! Bringing up a run's inference thread, shared by the CLI and the app: ONNX Runtime, CoreML's
//! compiled-model cache, the models CloakFrame 1.12.1 downloaded, and the models themselves.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use cloakframe_core::detection::ModelId;

use crate::detect::fallback::{Acceleration, Backends};
use crate::detect::models::{DetectionModels, LoadError};
use crate::detect::runtime::OrtRuntime;
use crate::detect::{Detector, DetectorHandle, NoDetections};
use crate::gpu_pack::GpuPack;
use crate::model_files::{InstallError, ModelStore, model_file};

/// The inference thread and where each model on it runs.
pub struct Started {
    pub handle: DetectorHandle,
    pub backends: Backends,
}

/// Something done or tried on the way that the user should hear of; none of it stops the run.
#[derive(Debug)]
pub enum StartNote {
    /// A model was copied into the app's model folder from 1.12.1's.
    Copied { model: ModelId, from: PathBuf },
    /// 1.12.1's compiled CoreML models could not be removed.
    LegacyCoremlCache(io::Error),
    /// Compiled CoreML models of model files this build does not use could not be removed.
    UnusedCoremlCaches(io::Error),
    /// The Linux GPU pack is installed but was not used, for this reason.
    GpuPackUnused(String),
}

impl fmt::Display for StartNote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Copied { model, from } => write!(
                f,
                "copied {} from {}",
                model_file(*model).name,
                from.parent().unwrap_or(from).display()
            ),
            Self::LegacyCoremlCache(e) => write!(
                f,
                "cannot remove the models CloakFrame 1.12.1 compiled for CoreML: {e}"
            ),
            Self::UnusedCoremlCaches(e) => write!(
                f,
                "cannot remove compiled CoreML models this build does not use: {e}"
            ),
            Self::GpuPackUnused(reason) => {
                write!(f, "the NVIDIA GPU pack is not used: {reason}")
            }
        }
    }
}

#[derive(Debug)]
pub enum StartError {
    /// A model 1.12.1 downloaded could not be copied into the app's model folder.
    Copy {
        model: ModelId,
        into: PathBuf,
        error: InstallError,
    },
    Runtime(String),
    Load(LoadError),
    Thread(io::Error),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Copy { model, into, error } => write!(
                f,
                "cannot copy {} into {}: {error}",
                model_file(*model).name,
                into.display()
            ),
            Self::Runtime(e) => f.write_str(e),
            Self::Load(e) => e.fmt(f),
            Self::Thread(e) => write!(f, "cannot start the inference thread: {e}"),
        }
    }
}

impl std::error::Error for StartError {}

pub fn spawn(detector: Box<dyn Detector>) -> Result<DetectorHandle, StartError> {
    DetectorHandle::spawn(detector).map_err(StartError::Thread)
}

/// For runs with nothing to analyze.
pub fn without_models() -> Result<Started, StartError> {
    Ok(Started {
        handle: spawn(Box::new(NoDetections))?,
        backends: Backends::default(),
    })
}

/// ONNX Runtime from the Linux GPU pack when one is installed in `gpu_pack`, otherwise from
/// `library`. A damaged pack, or one whose NVIDIA libraries do not load, is passed over with a
/// note; ORT can be loaded only once, so a pack that fails after loading it is an error.
pub fn load_runtime(
    library: &Path,
    gpu_pack: Option<&GpuPack>,
    note: &mut dyn FnMut(StartNote),
) -> Result<OrtRuntime, StartError> {
    #[cfg(target_os = "linux")]
    if let Some(pack) = gpu_pack {
        use crate::detect::runtime::RuntimeError;
        use crate::gpu_pack::Status;
        match pack.status() {
            Status::Missing => {}
            Status::Damaged(reason) => note(StartNote::GpuPackUnused(reason)),
            Status::Ready => match OrtRuntime::load_gpu_pack(&pack.dir()) {
                Ok(runtime) => return Ok(runtime),
                Err(RuntimeError::GpuPack(reason)) => note(StartNote::GpuPackUnused(reason)),
                Err(e) => return Err(StartError::Runtime(e.to_string())),
            },
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (gpu_pack, note);
    OrtRuntime::load(library).map_err(|e| StartError::Runtime(e.to_string()))
}

/// Loads ONNX Runtime (`load_runtime`) and `models` from `store`. The app's own model folder
/// (`app_folder`) first takes the models it lacks from 1.12.1's.
pub fn with_models(
    library: &Path,
    gpu_pack: Option<&GpuPack>,
    store: &ModelStore,
    app_folder: bool,
    models: DetectionModels,
    acceleration: Acceleration,
    mut note: impl FnMut(StartNote),
) -> Result<Started, StartError> {
    if app_folder {
        import_legacy(store, models.ids(), &mut note)?;
    }
    let runtime = load_runtime(library, gpu_pack, &mut note)?;
    load(runtime, store, models, acceleration, note)
}

/// Like [`with_models`] on a runtime the caller loaded: ORT allows one environment per process,
/// so a front end that starts more than one run keeps one and passes it to each.
pub fn with_runtime(
    runtime: OrtRuntime,
    store: &ModelStore,
    app_folder: bool,
    models: DetectionModels,
    acceleration: Acceleration,
    mut note: impl FnMut(StartNote),
) -> Result<Started, StartError> {
    if app_folder {
        import_legacy(store, models.ids(), &mut note)?;
    }
    load(runtime, store, models, acceleration, note)
}

fn load(
    runtime: OrtRuntime,
    store: &ModelStore,
    models: DetectionModels,
    acceleration: Acceleration,
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut, unused_variables))] mut note: impl FnMut(
        StartNote,
    ),
) -> Result<Started, StartError> {
    #[cfg(target_os = "macos")]
    let runtime = match crate::app_dirs::cache_dir() {
        Some(cache) => {
            if let Err(e) = crate::legacy::remove_coreml_cache(&cache) {
                note(StartNote::LegacyCoremlCache(e));
            }
            let runtime = runtime.with_coreml_cache(cache.join("coreml"));
            if let Err(e) = runtime.remove_unused_coreml_caches() {
                note(StartNote::UnusedCoremlCaches(e));
            }
            runtime
        }
        None => runtime,
    };
    let loaded = models
        .load(&runtime, store, acceleration)
        .map_err(StartError::Load)?;
    Ok(Started {
        handle: spawn(loaded.detector)?,
        backends: loaded.backends,
    })
}

/// Copies into the app's model folder the models it lacks that CloakFrame 1.12.1 downloaded.
pub fn import_legacy(
    store: &ModelStore,
    models: impl IntoIterator<Item = ModelId>,
    note: &mut impl FnMut(StartNote),
) -> Result<(), StartError> {
    let folders = crate::legacy::model_dirs();
    for model in models {
        match store.import(model, &folders) {
            Ok(Some(from)) => note(StartNote::Copied { model, from }),
            Ok(None) => {}
            Err(error) => {
                return Err(StartError::Copy {
                    model,
                    into: store.dir().to_path_buf(),
                    error,
                });
            }
        }
    }
    Ok(())
}
