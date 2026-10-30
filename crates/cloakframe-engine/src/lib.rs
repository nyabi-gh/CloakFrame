//! CloakFrame engine: runs jobs and owns every file, media, inference and network operation.

pub mod app_dirs;
pub mod batch;
pub mod cancel;
pub mod detect;
pub mod events;
pub mod fs_case;
pub mod identity;
pub mod legacy;
pub mod media;
pub mod media_processor;
pub mod model_files;
pub mod ordered;
pub mod output_plan;
pub mod photo;
pub mod publish;
pub mod review;
pub mod run;
pub mod scan;
pub mod session;
pub mod startup;
#[cfg(feature = "synthetic")]
pub mod synthetic;
pub mod update;
pub mod video;

#[cfg(test)]
#[path = "../tests/common/mod.rs"]
mod test_support;
