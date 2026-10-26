//! Runs the core mask renderer on decoded pixels.

use std::fmt;

use crate::run::{Classify, FailureKind};
use cloakframe_core::mask::{
    ImageMut, Layout, MaskError, MaskStyle, Overlay, Pixels, Region, render,
};
use image::DynamicImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyError {
    /// Floating-point samples, which decoding already refuses.
    UnsupportedSamples,
    Mask(MaskError),
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSamples => f.write_str("only 8- and 16-bit images can be masked"),
            Self::Mask(e) => e.fmt(f),
        }
    }
}

impl Classify for ApplyError {
    fn kind(&self) -> FailureKind {
        FailureKind::Other
    }
}

impl std::error::Error for ApplyError {}

/// Masks `regions` in place, keeping the image's sample type and channels.
pub fn apply(
    image: &mut DynamicImage,
    regions: &[Region],
    style: &MaskStyle,
    overlay: Option<&Overlay>,
) -> Result<(), ApplyError> {
    let (width, height) = (image.width(), image.height());
    let (layout, pixels) = match image {
        DynamicImage::ImageLuma8(i) => (Layout::Gray, Pixels::U8(i)),
        DynamicImage::ImageLumaA8(i) => (Layout::GrayAlpha, Pixels::U8(i)),
        DynamicImage::ImageRgb8(i) => (Layout::Rgb, Pixels::U8(i)),
        DynamicImage::ImageRgba8(i) => (Layout::Rgba, Pixels::U8(i)),
        DynamicImage::ImageLuma16(i) => (Layout::Gray, Pixels::U16(i)),
        DynamicImage::ImageLumaA16(i) => (Layout::GrayAlpha, Pixels::U16(i)),
        DynamicImage::ImageRgb16(i) => (Layout::Rgb, Pixels::U16(i)),
        DynamicImage::ImageRgba16(i) => (Layout::Rgba, Pixels::U16(i)),
        _ => return Err(ApplyError::UnsupportedSamples),
    };
    let mut view = ImageMut::new(width, height, layout, pixels).map_err(ApplyError::Mask)?;
    render(&mut view, regions, style, overlay).map_err(ApplyError::Mask)
}
