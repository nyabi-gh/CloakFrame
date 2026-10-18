//! Photos: reading the source once, deciding its format from content, limits, decoding in
//! display orientation, the color policy and the detection copy, masking, and encoding with the
//! output metadata (plan 4.3, steps 2–3 and 6).

pub mod budget;
pub mod color;
pub mod decode;
pub mod encode;
pub mod format;
pub mod mask;
pub mod metadata;
