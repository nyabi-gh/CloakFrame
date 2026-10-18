//! CloakFrame engine: runs jobs and owns every file, media, inference and network operation.

pub mod app_dirs;
pub mod batch;
pub mod cancel;
pub mod detect;
pub mod events;
pub mod fs_case;
pub mod identity;
pub mod ordered;
pub mod output_plan;
pub mod photo;
pub mod publish;
pub mod review;
pub mod run;
pub mod scan;
pub mod session;
#[cfg(feature = "synthetic")]
pub mod synthetic;

#[cfg(test)]
mod testdir;
