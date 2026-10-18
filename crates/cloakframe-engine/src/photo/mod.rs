//! Photos: reading the source once, deciding its format from content, limits, decoding in
//! display orientation, the color policy and the detection copy, masking, encoding with the
//! output metadata, and the processor that runs them for the runner (plan 4.3).

pub mod budget;
pub mod color;
pub mod decode;
pub mod encode;
pub mod format;
pub mod mask;
pub mod metadata;
pub mod process;
