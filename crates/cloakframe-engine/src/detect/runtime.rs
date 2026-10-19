//! ONNX Runtime loaded from an explicit library path, with telemetry off and the execution
//! provider named by the engine (plan 3, inference EP policy).

use std::fmt;
use std::path::Path;

use ort::environment::Environment;
use ort::ep;
use ort::session::Session;
use ort::session::builder::{AutoDevicePolicy, GraphOptimizationLevel, SessionBuilder};
use ort::value::DynValue;

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

/// The process's ONNX Runtime environment. ORT allows one per process, so it is loaded once and
/// shared by every session.
#[derive(Debug)]
pub struct OrtRuntime {
    env: Environment,
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
        Ok(Self { env })
    }

    /// A CPU session with 1.12.1's CPU options: every graph optimization and one intra-op
    /// thread per logical core.
    pub(crate) fn cpu_session(&self, model: &[u8]) -> ort::Result<Session> {
        self.cpu_session_with(model, Vec::new())
    }

    /// [`Self::cpu_session`] with the named initializers of the graph replaced before it is
    /// optimized.
    pub(crate) fn cpu_session_with(
        &self,
        model: &[u8],
        initializers: Vec<(&str, DynValue)>,
    ) -> ort::Result<Session> {
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        let option = |r: Result<SessionBuilder, ort::Error<SessionBuilder>>| {
            r.map_err(ort::Error::<()>::from)
        };
        let mut builder = Session::builder(&self.env)?;
        // ort's builder sets the MaxEfficiency policy, under which ORT 1.31 adds CoreML next to an
        // explicit CPU EP. Reset it before naming the EP.
        builder = option(builder.with_auto_device(AutoDevicePolicy::PreferCPU))?;
        builder = option(builder.with_optimization_level(GraphOptimizationLevel::All))?;
        builder = option(builder.with_intra_threads(threads))?;
        for (name, value) in initializers {
            builder = option(builder.with_initializer(name, value))?;
        }
        let cpu = ep::CPU::default().build().error_on_failure();
        option(builder.with_execution_providers([cpu]))?.commit_from_memory(model)
    }
}
