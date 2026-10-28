//! ONNX Runtime loaded from an explicit library path, with telemetry off and the execution
//! provider named by the engine (plan 3, inference EP policy).

use std::fmt;
use std::path::Path;
#[cfg(target_os = "macos")]
use std::path::PathBuf;

use ort::environment::Environment;
use ort::ep;
use ort::session::Session;
use ort::session::builder::{AutoDevicePolicy, GraphOptimizationLevel, SessionBuilder};
use ort::value::DynValue;
#[cfg(target_os = "macos")]
use sha2::{Digest as _, Sha256};

/// Microsoft's ORT builds send telemetry unless this is `1` when the library creates its
/// environment; `DisableTelemetryEvents` alone does not stop the upload (docs/M0_ORT_SPIKE.md).
pub const TELEMETRY_VARIABLE: &str = "ORT_DISABLE_TELEMETRY";

/// Sets [`TELEMETRY_VARIABLE`] for the process. The first statement of every `main` that may load
/// ONNX Runtime.
///
/// # Safety
///
/// No other thread may exist yet (`std::env::set_var`).
#[allow(unsafe_code)]
pub unsafe fn disable_telemetry() {
    // SAFETY: the caller guarantees the process is still single-threaded.
    unsafe { std::env::set_var(TELEMETRY_VARIABLE, "1") };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    /// [`disable_telemetry`] was not called before loading.
    TelemetryNotDisabled,
    Load(String),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TelemetryNotDisabled => write!(f, "{TELEMETRY_VARIABLE} is not set to 1"),
            Self::Load(reason) => write!(f, "could not load ONNX Runtime: {reason}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

/// Where a session runs. The accelerators are the ones the pinned runtimes carry (plan 3):
/// CoreML in the macOS build, DirectML in Windows ML.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Cpu,
    CoreMl,
    DirectMl,
}

impl Backend {
    /// This platform's accelerator, if its runtime has one.
    pub fn accelerator() -> Option<Self> {
        if cfg!(target_os = "macos") {
            Some(Self::CoreMl)
        } else if cfg!(windows) {
            Some(Self::DirectMl)
        } else {
            None
        }
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Cpu => "CPU",
            Self::CoreMl => "CoreML",
            Self::DirectMl => "DirectML",
        })
    }
}

/// The process's ONNX Runtime environment. ORT allows one per process, so it is loaded once and
/// shared by every session. Clones share it.
#[derive(Debug, Clone)]
pub struct OrtRuntime {
    env: Environment,
    cpu_spinning: bool,
    #[cfg(target_os = "macos")]
    coreml_cache: Option<PathBuf>,
}

impl OrtRuntime {
    /// Loads the library at `library` (never a system copy: Windows has an older ORT in System32)
    /// and refuses to when telemetry would stay on.
    pub fn load(library: &Path) -> Result<Self, RuntimeError> {
        if std::env::var_os(TELEMETRY_VARIABLE).is_none_or(|v| v != "1") {
            return Err(RuntimeError::TelemetryNotDisabled);
        }
        let load = |e: String| RuntimeError::Load(e);
        let env = ort::init_from(library)
            .map_err(|e| load(e.to_string()))?
            .with_name("CloakFrame")
            .with_telemetry(false)
            .build()
            .map_err(|e| load(e.to_string()))?;
        Ok(Self {
            env,
            cpu_spinning: false,
            #[cfg(target_os = "macos")]
            coreml_cache: None,
        })
    }

    /// Keeps CoreML's compiled models under `dir`. Without it, every CoreML session compiles its
    /// model again.
    #[cfg(target_os = "macos")]
    #[must_use]
    pub fn with_coreml_cache(mut self, dir: PathBuf) -> Self {
        self.coreml_cache = Some(dir);
        self
    }

    /// Lets CPU sessions' idle threads spin while they wait for work, as ORT does by default and
    /// 1.12.1 did. Off otherwise: the spinning threads took the cores the FFmpeg decode of a
    /// video's analysis and a photo worker's decode and encode run on, which made video
    /// detection 1.6–2 times and photo runs about 40% slower for the same results
    /// (docs/M5_PLAN.md). Kept to measure that on every platform.
    #[must_use]
    pub fn with_cpu_spinning(mut self, spinning: bool) -> Self {
        self.cpu_spinning = spinning;
        self
    }

    /// Removes from the CoreML cache folder what no session of this build uses: the folders of
    /// other ORT builds and, in this build's, of models it does not pin. Another build running at
    /// the same time loses its compiled models and compiles them again, or starts on the CPU if
    /// they go while it compiles. Returns how many entries were removed.
    #[cfg(target_os = "macos")]
    pub fn remove_unused_coreml_caches(&self) -> std::io::Result<usize> {
        let Some(root) = &self.coreml_cache else {
            return Ok(0);
        };
        let build = coreml_build_dir(root);
        let pinned: Vec<PathBuf> = crate::model_files::MODEL_FILES
            .iter()
            .map(|f| build.join(coreml_cache_name(f.sha256)))
            .collect();
        let mut removed = 0;
        for entry in entries(root)? {
            if entry == build {
                for model in entries(&build)? {
                    if !pinned.contains(&model) {
                        remove(&model)?;
                        removed += 1;
                    }
                }
            } else {
                remove(&entry)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// A session on `backend` with the named initializers of the graph replaced before it is
    /// optimized. The options are 1.12.1's but for spinning: on the CPU every graph optimization
    /// and one intra-op thread per logical core; on an accelerator the extended level, whose layout steps beyond
    /// it help only CPU nodes and can turn nodes into forms the accelerator no longer takes, and
    /// one thread. On CoreML a model's [`coreml_fixed_dimensions`] are fixed.
    pub(crate) fn session(
        &self,
        model: &[u8],
        backend: Backend,
        initializers: Vec<(&str, DynValue)>,
    ) -> ort::Result<Session> {
        let option = |r: Result<SessionBuilder, ort::Error<SessionBuilder>>| {
            r.map_err(ort::Error::<()>::from)
        };
        let mut builder = Session::builder(&self.env)?;
        // ort's builder sets the MaxEfficiency policy, under which ORT 1.31 adds CoreML next to an
        // explicit CPU EP. Reset it before naming the EP.
        builder = option(builder.with_auto_device(AutoDevicePolicy::PreferCPU))?;
        let (level, threads) = match backend {
            Backend::Cpu => (
                GraphOptimizationLevel::All,
                std::thread::available_parallelism().map_or(1, |n| n.get()),
            ),
            Backend::CoreMl | Backend::DirectMl => (GraphOptimizationLevel::Level2, 1),
        };
        builder = option(builder.with_optimization_level(level))?;
        builder = option(builder.with_intra_threads(threads))?;
        if backend == Backend::Cpu {
            builder = option(builder.with_intra_op_spinning(self.cpu_spinning))?;
            builder = option(builder.with_inter_op_spinning(self.cpu_spinning))?;
        }
        // A replaced initializer leaves the graph's structure as it was, so CoreML would load the
        // compiled model of the graph as published; such sessions are not cached.
        #[cfg(target_os = "macos")]
        let cache = self
            .coreml_cache
            .as_ref()
            .filter(|_| initializers.is_empty());
        for (name, value) in initializers {
            builder = option(builder.with_initializer(name, value))?;
        }
        let provider = match backend {
            Backend::Cpu => ep::CPU::default().build(),
            #[cfg(target_os = "macos")]
            Backend::CoreMl => {
                let model_sha256 = hex(&Sha256::digest(model));
                for &(name, size) in coreml_fixed_dimensions(&model_sha256) {
                    builder = option(builder.with_dimension_override(name, size))?;
                }
                let mut coreml = ep::CoreML::default()
                    .with_model_format(ep::coreml::ModelFormat::MLProgram)
                    .with_compute_units(ep::coreml::ComputeUnits::All);
                if let Some(dir) = cache {
                    let dir = coreml_build_dir(dir).join(coreml_cache_name(&model_sha256));
                    std::fs::create_dir_all(&dir)
                        .map_err(|e| ort::Error::new(format!("{}: {e}", dir.display())))?;
                    coreml = coreml.with_model_cache_dir(dir.display());
                }
                coreml.build()
            }
            #[cfg(windows)]
            Backend::DirectMl => {
                builder = option(builder.with_memory_pattern(false))?;
                builder = option(builder.with_parallel_execution(false))?;
                ep::DirectML::default().with_device_id(0).build()
            }
            other => return Err(ort::Error::new(format!("{other} is not in this build"))),
        };
        option(builder.with_execution_providers([provider.error_on_failure()]))?
            .commit_from_memory(model)
    }
}

/// The free dimensions fixed in the CoreML sessions of the model with this SHA-256. Only there: on
/// the CPU a fixed shape gains nothing and moves scores in the sixth decimal, which changes
/// recorded detections.
#[cfg(target_os = "macos")]
fn coreml_fixed_dimensions(model_sha256: &str) -> &'static [(&'static str, i64)] {
    if model_sha256 == super::yolo5face::MODEL_SHA256 {
        super::yolo5face::FIXED_DIMENSIONS
    } else {
        &[]
    }
}

/// One folder per ORT build and model. CoreML reuses a compiled model while the graph's structure
/// is unchanged, even when its weights or the runtime that partitioned it changed. Fixed
/// dimensions change the partitions but not the graph, so they are part of the name.
#[cfg(target_os = "macos")]
fn coreml_cache_name(model_sha256: &str) -> String {
    coreml_fixed_dimensions(model_sha256)
        .iter()
        .fold(model_sha256.to_owned(), |name, (dimension, size)| {
            format!("{name}-{dimension}{size}")
        })
}

#[cfg(target_os = "macos")]
fn coreml_build_dir(root: &Path) -> PathBuf {
    root.join(&hex(&Sha256::digest(ort::info().as_bytes()))[..16])
}

#[cfg(target_os = "macos")]
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The paths in `dir`, none if it does not exist.
#[cfg(target_os = "macos")]
fn entries(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries.map(|e| e.map(|e| e.path())).collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

#[cfg(target_os = "macos")]
fn remove(path: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}
