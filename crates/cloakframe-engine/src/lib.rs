//! CloakFrame engine: runs jobs and owns every file, media, inference and network operation.

pub mod cancel;
pub mod detect;
pub mod events;
pub mod fs_case;
pub mod identity;
pub mod ordered;
pub mod run;
pub mod scan;

#[cfg(test)]
mod testdir;
