use std::borrow::Cow;
use std::fmt;
use std::io::Cursor;

use cloakframe_core::mask::Overlay;
use image::{DynamicImage, ImageBuffer, ImageDecoder};
use moxcms::{
    ColorProfile, DataColorSpace, Layout, RenderingIntent, ToneReprCurve, TransformOptions, Xyzd,
};

use crate::detect::Rgb8Image;
use crate::photo::decode::{self, Source, detection_rgb8};
use crate::photo::format::ImageFormat;

/// Color spaces whose pixels are kept as they are, with a bundled profile attached (D2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standard {
    Srgb,
    DisplayP3,
    AdobeRgb,
}

impl Standard {
    const ALL: [Self; 3] = [Self::Srgb, Self::DisplayP3, Self::AdobeRgb];

    fn reference(self) -> ColorProfile {
        match self {
            Self::Srgb => ColorProfile::new_srgb(),
            Self::DisplayP3 => ColorProfile::new_display_p3(),
            Self::AdobeRgb => ColorProfile::new_adobe_rgb(),
        }
    }

    /// The profile written into outputs (`tools/icc/make_profiles.py`): ICC v2.1 built from the
    /// published primaries. moxcms can encode profiles, but its files fail libpng's checks (tag
    /// offsets and length not 4-byte aligned), so they are not used for output.
    pub fn profile_bytes(self) -> &'static [u8] {
        match self {
            Self::Srgb => include_bytes!("../../../../assets/icc/srgb.icc"),
            Self::DisplayP3 => include_bytes!("../../../../assets/icc/display-p3.icc"),
            Self::AdobeRgb => include_bytes!("../../../../assets/icc/adobe-rgb.icc"),
        }
    }
}

/// What the color policy did to an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorOutcome {
    /// No profile: the pixels are treated as sRGB and nothing is attached.
    Untagged,
    /// Pixels unchanged; the bundled profile is attached on output.
    Kept(Standard),
    /// Pixels converted to sRGB from another profile; the output is marked as color-converted.
    Converted,
    /// A profile that could not be used (unreadable, or not matching the pixels). It is dropped
    /// and the pixels are treated as sRGB; the result says so.
    Ignored(IgnoredProfile),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnoredProfile {
    Unreadable,
    NotForTheseSamples,
}

impl ColorOutcome {
    /// The profile to embed in the output, if any.
    pub fn embed(self) -> Option<Standard> {
        match self {
            Self::Kept(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct ColorError(String);

impl fmt::Display for ColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "color conversion failed: {}", self.0)
    }
}

impl std::error::Error for ColorError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddedIcc {
    None,
    Profile(Vec<u8>),
    /// The container has a profile chunk whose contents could not be read.
    Unreadable,
}

/// The ICC profile embedded in the source, from the bytes already read.
pub fn embedded_icc(source: &Source) -> EmbeddedIcc {
    if source.format == ImageFormat::Bmp {
        return bmp_icc(&source.bytes);
    }
    let found = read_icc(source);
    match found {
        Some(profile) => EmbeddedIcc::Profile(profile),
        None if has_icc_chunk(source) => EmbeddedIcc::Unreadable,
        None => EmbeddedIcc::None,
    }
}

/// Whether PNG `iCCP` or WebP `ICCP` is present. image drops a damaged profile chunk without an
/// error, which would otherwise look like an untagged image.
fn has_icc_chunk(source: &Source) -> bool {
    let b = &source.bytes;
    match source.format {
        ImageFormat::Png => {
            let mut at = 8;
            while let (Some(len), Some(kind)) = (
                b.get(at..at + 4)
                    .map(|l| u32::from_be_bytes(l.try_into().unwrap()) as usize),
                b.get(at + 4..at + 8),
            ) {
                match kind {
                    b"iCCP" => return true,
                    b"IDAT" | b"IEND" => return false,
                    _ => at = at.saturating_add(12).saturating_add(len),
                }
            }
            false
        }
        ImageFormat::Webp => {
            let mut at = 12;
            while let (Some(kind), Some(len)) = (
                b.get(at..at + 4),
                b.get(at + 4..at + 8)
                    .map(|l| u32::from_le_bytes(l.try_into().unwrap()) as usize),
            ) {
                if kind == b"ICCP" {
                    return true;
                }
                at = at
                    .saturating_add(8)
                    .saturating_add(len)
                    .saturating_add(len & 1);
            }
            false
        }
        _ => false,
    }
}

fn read_icc(source: &Source) -> Option<Vec<u8>> {
    match source.format {
        ImageFormat::Jpeg => return jpeg_icc(&source.bytes),
        ImageFormat::Tiff => return tiff_icc(&source.bytes),
        ImageFormat::Png | ImageFormat::Webp => {}
        ImageFormat::Bmp => unreachable!("BMP profiles are read by bmp_icc"),
    }
    let mut decoder = decode::reader(&source.bytes, source.format, 0)
        .into_decoder()
        .ok()?;
    decoder
        .icc_profile()
        .ok()
        .flatten()
        .filter(|p| !p.is_empty() && p.len() as u64 <= decode::MAX_ICC_BYTES)
}

/// Tag 34675 of the first IFD. image sizes its TIFF limits from the pixel buffer, and a tag value
/// is held as one 24-byte value per byte while it is read, so on small images it drops the
/// profile as over the limit.
fn tiff_icc(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut limits = tiff::decoder::Limits::default();
    limits.decoding_buffer_size = MAX_TIFF_ICC_READ;
    limits.ifd_value_size = MAX_TIFF_ICC_READ;
    tiff::decoder::Decoder::new(Cursor::new(bytes))
        .ok()?
        .with_limits(limits)
        .get_tag_u8_vec(tiff::tags::Tag::IccProfile)
        .ok()
        .filter(|p| !p.is_empty())
}

/// Room for a profile of a few megabytes at the decoder's per-byte cost.
const MAX_TIFF_ICC_READ: usize = 128 << 20;

/// APP2 `ICC_PROFILE` chunks, joined in their sequence order.
fn jpeg_icc(bytes: &[u8]) -> Option<Vec<u8>> {
    const TAG: &[u8] = b"ICC_PROFILE\0";
    let mut chunks: Vec<(u8, &[u8])> = Vec::new();
    let mut at = 2;
    while at + 4 <= bytes.len() && bytes[at] == 0xFF {
        let marker = bytes[at + 1];
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        if marker == 0xFF {
            at += 1;
            continue;
        }
        let len = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        let segment = bytes.get(at + 4..at + 2 + len)?;
        if marker == 0xE2 && segment.starts_with(TAG) && segment.len() > TAG.len() + 2 {
            chunks.push((segment[TAG.len()], &segment[TAG.len() + 2..]));
        }
        at += 2 + len;
    }
    if chunks.is_empty() {
        return None;
    }
    chunks.sort_by_key(|(seq, _)| *seq);
    Some(
        chunks
            .into_iter()
            .flat_map(|(_, data)| data.iter().copied())
            .collect(),
    )
}

const BMP_FILE_HEADER: usize = 14;
const BMP_V5_HEADER: usize = 124;
const BMP_PROFILE_LINKED: u32 = 0x4C49_4E4B;
const BMP_PROFILE_EMBEDDED: u32 = 0x4D42_4544;

/// The profile a BITMAPV5HEADER embeds. image reads none from BMP. A linked profile names another
/// file, which is not read (plan 4.3), so it counts as unreadable; the other color space types
/// (sRGB, Windows default, calibrated endpoints) are treated as sRGB, as 1.12.1 did.
fn bmp_icc(bytes: &[u8]) -> EmbeddedIcc {
    let le32 = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
    };
    let header = BMP_FILE_HEADER;
    if le32(header).is_none_or(|size| size < BMP_V5_HEADER) {
        return EmbeddedIcc::None;
    }
    match le32(header + 56).map(|t| t as u32) {
        Some(BMP_PROFILE_EMBEDDED) => {}
        Some(BMP_PROFILE_LINKED) => return EmbeddedIcc::Unreadable,
        _ => return EmbeddedIcc::None,
    }
    let profile = le32(header + 112)
        .zip(le32(header + 116))
        .and_then(|(offset, size)| {
            let start = header.checked_add(offset)?;
            bytes.get(start..start.checked_add(size)?)
        })
        .filter(|p| !p.is_empty());
    match profile {
        Some(p) => EmbeddedIcc::Profile(p.to_vec()),
        None => EmbeddedIcc::Unreadable,
    }
}

/// Colorants are compared after D50 adaptation, as profiles store them.
const COLORANT_TOLERANCE: f64 = 0.002;
/// Pure-gamma curves compare their exponent; other curves compare their values at 0..=1.
const GAMMA_TOLERANCE: f32 = 0.01;
const CURVE_TOLERANCE: f32 = 0.002;

/// Decides by colorimetry, not by bytes or names: files from different vendors (and the
/// fixtures' generated profiles) describe the same space with different bytes.
pub fn identify(profile: &ColorProfile) -> Option<Standard> {
    if profile.color_space != DataColorSpace::Rgb {
        return None;
    }
    Standard::ALL.into_iter().find(|s| {
        let reference = s.reference();
        let close = |a: &Xyzd, b: &Xyzd| {
            (a.x - b.x).abs() <= COLORANT_TOLERANCE
                && (a.y - b.y).abs() <= COLORANT_TOLERANCE
                && (a.z - b.z).abs() <= COLORANT_TOLERANCE
        };
        close(&profile.red_colorant, &reference.red_colorant)
            && close(&profile.green_colorant, &reference.green_colorant)
            && close(&profile.blue_colorant, &reference.blue_colorant)
            && [&profile.red_trc, &profile.green_trc, &profile.blue_trc]
                .iter()
                .all(|trc| {
                    trc.as_ref()
                        .is_some_and(|t| same_curve(t, reference.red_trc.as_ref().unwrap()))
                })
    })
}

fn pure_gamma(curve: &ToneReprCurve) -> Option<f32> {
    match curve {
        ToneReprCurve::Lut(lut) if lut.len() == 1 => Some(f32::from(lut[0]) / 256.0),
        ToneReprCurve::Parametric(p) if p.len() == 1 => Some(p[0]),
        _ => None,
    }
}

fn same_curve(a: &ToneReprCurve, b: &ToneReprCurve) -> bool {
    if let (Some(ga), Some(gb)) = (pure_gamma(a), pure_gamma(b)) {
        return (ga - gb).abs() <= GAMMA_TOLERANCE;
    }
    let (Ok(ea), Ok(eb)) = (a.make_linear_evaluator(), b.make_linear_evaluator()) else {
        return false;
    };
    (0..=64).all(|i| {
        let x = i as f32 / 64.0;
        (ea.evaluate_value(x) - eb.evaluate_value(x)).abs() <= CURVE_TOLERANCE
    })
}

/// Applies the color policy in place: known spaces are kept, other RGB profiles are converted
/// to sRGB and gray profiles to the sRGB curve (relative colorimetric), and anything unusable is
/// dropped with a note.
pub fn apply(source: &Source, image: &mut DynamicImage) -> Result<ColorOutcome, ColorError> {
    let icc = match embedded_icc(source) {
        EmbeddedIcc::None => return Ok(ColorOutcome::Untagged),
        EmbeddedIcc::Unreadable => return Ok(ColorOutcome::Ignored(IgnoredProfile::Unreadable)),
        EmbeddedIcc::Profile(icc) => icc,
    };
    let Ok(profile) = ColorProfile::new_from_slice(&icc) else {
        return Ok(ColorOutcome::Ignored(IgnoredProfile::Unreadable));
    };
    let rgb_image = matches!(
        image,
        DynamicImage::ImageRgb8(_)
            | DynamicImage::ImageRgba8(_)
            | DynamicImage::ImageRgb16(_)
            | DynamicImage::ImageRgba16(_)
    );
    if profile.color_space == DataColorSpace::Gray && !rgb_image {
        gray_to_srgb_curve(&profile, image)?;
        return Ok(ColorOutcome::Converted);
    }
    if profile.color_space != DataColorSpace::Rgb || !rgb_image {
        return Ok(ColorOutcome::Ignored(IgnoredProfile::NotForTheseSamples));
    }
    if let Some(standard) = identify(&profile) {
        return Ok(ColorOutcome::Kept(standard));
    }
    to_srgb(&profile, image)?;
    Ok(ColorOutcome::Converted)
}

/// Display P3 and Adobe RGB pixels converted to sRGB for a BMP output. A BITMAPV5HEADER can
/// embed a profile, but macOS ImageIO (Preview, Quick Look) does not open such files, ImageMagick's
/// included, so BMP outputs stay untagged sRGB.
pub fn srgb_for_bmp(
    image: &mut DynamicImage,
    outcome: ColorOutcome,
) -> Result<ColorOutcome, ColorError> {
    match outcome {
        ColorOutcome::Kept(standard @ (Standard::DisplayP3 | Standard::AdobeRgb)) => {
            to_srgb(&standard.reference(), image)?;
            Ok(ColorOutcome::Converted)
        }
        other => Ok(other),
    }
}

fn options() -> TransformOptions {
    TransformOptions {
        rendering_intent: RenderingIntent::RelativeColorimetric,
        prefer_fixed_point: false,
        ..TransformOptions::default()
    }
}

fn to_srgb(from: &ColorProfile, image: &mut DynamicImage) -> Result<(), ColorError> {
    let srgb = ColorProfile::new_srgb();
    let failed = |e: moxcms::CmsError| ColorError(e.to_string());
    match image {
        DynamicImage::ImageRgb8(buffer) => {
            let t = from
                .create_transform_8bit(Layout::Rgb, &srgb, Layout::Rgb, options())
                .map_err(failed)?;
            transform_in_place(buffer, |src, dst| t.transform(src, dst)).map_err(failed)
        }
        DynamicImage::ImageRgba8(buffer) => {
            let t = from
                .create_transform_8bit(Layout::Rgba, &srgb, Layout::Rgba, options())
                .map_err(failed)?;
            transform_in_place(buffer, |src, dst| t.transform(src, dst)).map_err(failed)
        }
        DynamicImage::ImageRgb16(buffer) => {
            let t = from
                .create_transform_16bit(Layout::Rgb, &srgb, Layout::Rgb, options())
                .map_err(failed)?;
            transform_in_place(buffer, |src, dst| t.transform(src, dst)).map_err(failed)
        }
        DynamicImage::ImageRgba16(buffer) => {
            let t = from
                .create_transform_16bit(Layout::Rgba, &srgb, Layout::Rgba, options())
                .map_err(failed)?;
            transform_in_place(buffer, |src, dst| t.transform(src, dst)).map_err(failed)
        }
        _ => unreachable!("apply only converts RGB images"),
    }
}

/// Gray stays gray: an sRGB gray is the sRGB curve alone, which untagged outputs are read with.
fn gray_to_srgb_curve(from: &ColorProfile, image: &mut DynamicImage) -> Result<(), ColorError> {
    let mut srgb_gray = ColorProfile::new_gray_with_gamma(1.0);
    srgb_gray.gray_trc = ColorProfile::new_srgb().red_trc;
    let failed = |e: moxcms::CmsError| ColorError(e.to_string());
    match image {
        DynamicImage::ImageLuma8(buffer) => {
            let t = from
                .create_transform_8bit(Layout::Gray, &srgb_gray, Layout::Gray, options())
                .map_err(failed)?;
            transform_in_place(buffer, |src, dst| t.transform(src, dst)).map_err(failed)
        }
        DynamicImage::ImageLumaA8(buffer) => {
            let t = from
                .create_transform_8bit(Layout::GrayAlpha, &srgb_gray, Layout::GrayAlpha, options())
                .map_err(failed)?;
            transform_in_place(buffer, |src, dst| t.transform(src, dst)).map_err(failed)
        }
        DynamicImage::ImageLuma16(buffer) => {
            let t = from
                .create_transform_16bit(Layout::Gray, &srgb_gray, Layout::Gray, options())
                .map_err(failed)?;
            transform_in_place(buffer, |src, dst| t.transform(src, dst)).map_err(failed)
        }
        DynamicImage::ImageLumaA16(buffer) => {
            let t = from
                .create_transform_16bit(Layout::GrayAlpha, &srgb_gray, Layout::GrayAlpha, options())
                .map_err(failed)?;
            transform_in_place(buffer, |src, dst| t.transform(src, dst)).map_err(failed)
        }
        _ => unreachable!("apply converts gray profiles only on gray images"),
    }
}

fn transform_in_place<P: image::Pixel>(
    buffer: &mut ImageBuffer<P, Vec<P::Subpixel>>,
    run: impl Fn(&[P::Subpixel], &mut [P::Subpixel]) -> Result<(), moxcms::CmsError>,
) -> Result<(), moxcms::CmsError> {
    let source = buffer.as_raw().clone();
    run(&source, buffer)
}

/// The user's image, which is sRGB, in the space of a photo whose pixels are kept in Display P3
/// or Adobe RGB, so it is drawn with the colors it was made with. Alpha is unchanged.
pub fn overlay_for(
    overlay: &Overlay,
    outcome: ColorOutcome,
) -> Result<Cow<'_, Overlay>, ColorError> {
    let Some(standard @ (Standard::DisplayP3 | Standard::AdobeRgb)) = outcome.embed() else {
        return Ok(Cow::Borrowed(overlay));
    };
    let t = ColorProfile::new_srgb()
        .create_transform_8bit(Layout::Rgba, &standard.reference(), Layout::Rgba, options())
        .map_err(|e| ColorError(e.to_string()))?;
    let mut rgba = vec![0; overlay.rgba().len()];
    t.transform(overlay.rgba(), &mut rgba)
        .map_err(|e| ColorError(e.to_string()))?;
    Overlay::new(overlay.width(), overlay.height(), rgba)
        .map(Cow::Owned)
        .map_err(|e| ColorError(e.to_string()))
}

/// The detection copy in sRGB. Kept Display P3 and Adobe RGB pixels are converted for the copy
/// only; the stored pixels stay in their space.
pub fn detection_srgb8(
    image: &DynamicImage,
    outcome: ColorOutcome,
) -> Result<Rgb8Image, ColorError> {
    let copy = detection_rgb8(image);
    let Some(standard @ (Standard::DisplayP3 | Standard::AdobeRgb)) = outcome.embed() else {
        return Ok(copy);
    };
    let mut rgb = image::RgbImage::from_raw(copy.width, copy.height, copy.data)
        .expect("detection_rgb8 returns a full buffer");
    let t = standard
        .reference()
        .create_transform_8bit(
            Layout::Rgb,
            &ColorProfile::new_srgb(),
            Layout::Rgb,
            options(),
        )
        .map_err(|e| ColorError(e.to_string()))?;
    transform_in_place(&mut rgb, |src, dst| t.transform(src, dst))
        .map_err(|e| ColorError(e.to_string()))?;
    Ok(Rgb8Image {
        width: rgb.width(),
        height: rgb.height(),
        data: rgb.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel::CancelToken;
    use crate::photo::budget::MemoryBudget;
    use crate::photo::decode::{decode, read_source};
    use std::path::{Path, PathBuf};

    fn d2(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/defects/D2")
            .join(name)
    }

    fn load(name: &str) -> (Source, DynamicImage) {
        let source = read_source(&d2(name)).unwrap();
        let image = decode(&source, &MemoryBudget::new(1 << 30), &CancelToken::new())
            .unwrap()
            .image;
        (source, image)
    }

    fn max_diff(a: &DynamicImage, b: &DynamicImage) -> u8 {
        let (a, b) = (a.to_rgb8(), b.to_rgb8());
        assert_eq!(a.dimensions(), b.dimensions());
        a.as_raw()
            .iter()
            .zip(b.as_raw())
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap()
    }

    #[test]
    fn identifies_the_fixture_profiles_by_colorimetry() {
        let identified = |name: &str| {
            identify(&ColorProfile::new_from_slice(&std::fs::read(d2(name)).unwrap()).unwrap())
        };
        assert_eq!(identified("srgb.icc"), Some(Standard::Srgb));
        assert_eq!(identified("display-p3.icc"), Some(Standard::DisplayP3));
        assert_eq!(identified("adobe-rgb.icc"), Some(Standard::AdobeRgb));
        assert_eq!(identified("wide-gamma18.icc"), None);
    }

    #[test]
    fn bundled_profiles_are_aligned_and_identify_as_their_own_space() {
        for standard in Standard::ALL {
            let bytes = standard.profile_bytes();
            let declared = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
            assert_eq!(declared, bytes.len());
            assert_eq!(bytes.len() % 4, 0, "{standard:?}");
            let count = u32::from_be_bytes(bytes[128..132].try_into().unwrap()) as usize;
            for tag in bytes[132..132 + 12 * count].chunks(12) {
                let offset = u32::from_be_bytes(tag[4..8].try_into().unwrap());
                assert_eq!(offset % 4, 0, "{standard:?} tag offset");
            }
            let parsed = ColorProfile::new_from_slice(bytes).unwrap();
            assert_eq!(identify(&parsed), Some(standard));
        }
    }

    #[test]
    fn known_spaces_keep_their_pixels_and_untagged_images_stay_untagged() {
        for (name, expected) in [
            ("chart-srgb.png", ColorOutcome::Kept(Standard::Srgb)),
            ("chart-srgb.jpg", ColorOutcome::Kept(Standard::Srgb)),
            (
                "chart-display-p3.png",
                ColorOutcome::Kept(Standard::DisplayP3),
            ),
            (
                "chart-display-p3.jpg",
                ColorOutcome::Kept(Standard::DisplayP3),
            ),
            (
                "chart-adobe-rgb.png",
                ColorOutcome::Kept(Standard::AdobeRgb),
            ),
            (
                "chart-adobe-rgb.jpg",
                ColorOutcome::Kept(Standard::AdobeRgb),
            ),
            ("chart-untagged.png", ColorOutcome::Untagged),
        ] {
            let (source, mut image) = load(name);
            let before = image.clone();
            assert_eq!(apply(&source, &mut image).unwrap(), expected, "{name}");
            assert_eq!(image, before, "{name} pixels changed");
        }
    }

    #[test]
    fn other_profiles_are_converted_to_srgb_like_little_cms() {
        let reference = image::open(d2("expected-wide-gamma18-as-srgb.png")).unwrap();
        for (name, tolerance) in [("chart-wide-gamma18.png", 2), ("chart-wide-gamma18.jpg", 4)] {
            let (source, mut image) = load(name);
            assert_eq!(apply(&source, &mut image).unwrap(), ColorOutcome::Converted);
            let diff = max_diff(&image, &reference);
            assert!(diff <= tolerance, "{name}: max channel difference {diff}");
        }
    }

    #[test]
    fn the_detection_copy_is_srgb_without_touching_the_stored_pixels() {
        for (name, reference) in [
            ("chart-display-p3.png", "expected-display-p3-as-srgb.png"),
            ("chart-adobe-rgb.png", "expected-adobe-rgb-as-srgb.png"),
        ] {
            let (source, mut image) = load(name);
            let outcome = apply(&source, &mut image).unwrap();
            let stored = image.clone();
            let copy = detection_srgb8(&image, outcome).unwrap();
            assert_eq!(image, stored, "{name}");
            let copy = DynamicImage::ImageRgb8(
                image::RgbImage::from_raw(copy.width, copy.height, copy.data).unwrap(),
            );
            let reference = image::open(d2(reference)).unwrap();
            let diff = max_diff(&copy, &reference);
            assert!(diff <= 2, "{name}: {diff} from Little CMS");
        }
        let (source, mut image) = load("chart-untagged.png");
        let outcome = apply(&source, &mut image).unwrap();
        assert_eq!(
            detection_srgb8(&image, outcome).unwrap().data,
            detection_rgb8(&image).data
        );
    }

    #[test]
    fn unusable_profiles_are_dropped_with_a_note() {
        let (mut source, mut image) = load("chart-srgb.png");
        let gray = image::DynamicImage::ImageLuma8(image.to_luma8());
        let mut gray_image = gray.clone();
        assert_eq!(
            apply(&source, &mut gray_image).unwrap(),
            ColorOutcome::Ignored(IgnoredProfile::NotForTheseSamples)
        );
        assert_eq!(gray_image, gray);
        let at = source.bytes.windows(4).position(|w| w == b"iCCP").unwrap();
        source.bytes[at + 20] ^= 0xFF;
        assert_eq!(
            apply(&source, &mut image).unwrap(),
            ColorOutcome::Ignored(IgnoredProfile::Unreadable)
        );

        let (mut jpeg, mut image) = load("chart-srgb.jpg");
        let at = jpeg
            .bytes
            .windows(12)
            .position(|w| w == b"ICC_PROFILE\0")
            .unwrap();
        jpeg.bytes[at + 14..at + 54].fill(0);
        assert_eq!(
            apply(&jpeg, &mut image).unwrap(),
            ColorOutcome::Ignored(IgnoredProfile::Unreadable)
        );
    }

    /// Gray profiles (macOS "Generic Gray Gamma 2.2", dot gain profiles) describe gray pixels;
    /// they are converted to the sRGB curve like other profiles.
    #[test]
    fn gray_profiles_convert_gray_pixels_to_the_srgb_curve() {
        let icc = ColorProfile::new_gray_with_gamma(1.8).encode().unwrap();
        let dir = crate::testdir::TestDir::new();
        let ramp = image::GrayImage::from_fn(256, 1, |x, _| image::Luma([x as u8]));
        let srgb = |v: f64| {
            let l = (v / 255.0).powf(1.8);
            let e = if l <= 0.003_130_8 {
                12.92 * l
            } else {
                1.055 * l.powf(1.0 / 2.4) - 0.055
            };
            e * 255.0
        };
        for image in [
            DynamicImage::ImageLuma8(ramp.clone()),
            DynamicImage::ImageLumaA8(DynamicImage::ImageLuma8(ramp.clone()).to_luma_alpha8()),
            DynamicImage::ImageLuma16(DynamicImage::ImageLuma8(ramp.clone()).to_luma16()),
        ] {
            let mut png = std::io::Cursor::new(Vec::new());
            let mut encoder = image::codecs::png::PngEncoder::new(&mut png);
            image::ImageEncoder::set_icc_profile(&mut encoder, icc.clone()).unwrap();
            image.write_with_encoder(encoder).unwrap();
            let path = dir.path().join("gray.png");
            std::fs::write(&path, png.into_inner()).unwrap();
            let source = read_source(&path).unwrap();
            let mut converted = image.clone();
            assert_eq!(
                apply(&source, &mut converted).unwrap(),
                ColorOutcome::Converted
            );
            assert_eq!(converted.color(), image.color());
            let got = converted.to_luma8();
            for x in [0u32, 16, 64, 128, 200, 255] {
                let want = srgb(f64::from(x));
                let v = f64::from(got.get_pixel(x, 0).0[0]);
                assert!(
                    (v - want).abs() <= 1.0,
                    "{:?} {x}: {v} vs {want}",
                    image.color()
                );
            }
            if let DynamicImage::ImageLumaA8(a) = &converted {
                assert!(a.pixels().all(|p| p.0[1] == 255));
            }
        }
    }

    /// image's 40-byte BMP header rewritten as a BITMAPV5HEADER embedding `icc` after the pixels.
    fn with_v5_profile(bmp: &[u8], icc: &[u8]) -> Vec<u8> {
        let le32 = |at: usize| u32::from_le_bytes(bmp[at..at + 4].try_into().unwrap());
        assert_eq!(le32(BMP_FILE_HEADER), 40);
        let rest = &bmp[BMP_FILE_HEADER + 40..];
        let mut header = bmp[BMP_FILE_HEADER..BMP_FILE_HEADER + 40].to_vec();
        header.resize(BMP_V5_HEADER, 0);
        header[0..4].copy_from_slice(&(BMP_V5_HEADER as u32).to_le_bytes());
        header[56..60].copy_from_slice(&BMP_PROFILE_EMBEDDED.to_le_bytes());
        header[112..116].copy_from_slice(&((BMP_V5_HEADER + rest.len()) as u32).to_le_bytes());
        header[116..120].copy_from_slice(&(icc.len() as u32).to_le_bytes());
        let total = BMP_FILE_HEADER + BMP_V5_HEADER + rest.len() + icc.len();
        let mut out = b"BM".to_vec();
        out.extend((total as u32).to_le_bytes());
        out.extend(&bmp[6..10]);
        out.extend((le32(10) + 84).to_le_bytes());
        out.extend(header);
        out.extend(rest);
        out.extend(icc);
        out
    }

    fn bmp_source(icc: Option<&[u8]>) -> (Source, DynamicImage) {
        let (png, image) = load("chart-display-p3.png");
        let mut plain = Cursor::new(Vec::new());
        image.write_to(&mut plain, image::ImageFormat::Bmp).unwrap();
        let bytes = match icc {
            Some(p) => with_v5_profile(plain.get_ref(), p),
            None => plain.into_inner(),
        };
        let source = Source {
            bytes,
            format: ImageFormat::Bmp,
            ..png
        };
        let decoded = decode(&source, &MemoryBudget::new(1 << 30), &CancelToken::new())
            .unwrap()
            .image;
        assert_eq!(decoded, image);
        (source, decoded)
    }

    #[test]
    fn bmp_v5_embedded_profiles_follow_the_policy() {
        let p3 = std::fs::read(d2("display-p3.icc")).unwrap();
        let (source, mut image) = bmp_source(Some(&p3));
        assert_eq!(embedded_icc(&source), EmbeddedIcc::Profile(p3.clone()));
        let before = image.clone();
        assert_eq!(
            apply(&source, &mut image).unwrap(),
            ColorOutcome::Kept(Standard::DisplayP3)
        );
        assert_eq!(image, before);

        let wide = std::fs::read(d2("wide-gamma18.icc")).unwrap();
        let (source, mut image) = bmp_source(Some(&wide));
        assert_eq!(apply(&source, &mut image).unwrap(), ColorOutcome::Converted);

        let (source, mut image) = bmp_source(None);
        assert_eq!(apply(&source, &mut image).unwrap(), ColorOutcome::Untagged);

        let (mut linked, mut image) = bmp_source(Some(&p3));
        linked.bytes[14 + 56..14 + 60].copy_from_slice(&BMP_PROFILE_LINKED.to_le_bytes());
        assert_eq!(
            apply(&linked, &mut image).unwrap(),
            ColorOutcome::Ignored(IgnoredProfile::Unreadable)
        );
        let (mut cut, _) = bmp_source(Some(&p3));
        cut.bytes.truncate(cut.bytes.len() - 1);
        assert_eq!(embedded_icc(&cut), EmbeddedIcc::Unreadable);
    }

    #[test]
    fn bmp_outputs_get_srgb_pixels() {
        for space in ["display-p3", "adobe-rgb"] {
            let (source, mut image) = load(&format!("chart-{space}.png"));
            let kept = apply(&source, &mut image).unwrap();
            assert_eq!(
                srgb_for_bmp(&mut image, kept).unwrap(),
                ColorOutcome::Converted
            );
            let reference = image::open(d2(&format!("expected-{space}-as-srgb.png"))).unwrap();
            let diff = max_diff(&image, &reference);
            assert!(diff <= 2, "{space}: {diff} from Little CMS");
        }
        let (source, mut image) = load("chart-srgb.png");
        let before = image.clone();
        let kept = apply(&source, &mut image).unwrap();
        assert_eq!(srgb_for_bmp(&mut image, kept).unwrap(), kept);
        assert_eq!(image, before);
    }

    #[test]
    fn small_tiffs_keep_their_profile() {
        // 8x8 RGB: image's own TIFF limits would drop the 2.5 KB profile.
        let mut bytes = Vec::new();
        let mut encoder = image::codecs::tiff::TiffEncoder::new(Cursor::new(&mut bytes));
        let p3 = Standard::DisplayP3.profile_bytes().to_vec();
        image::ImageEncoder::set_icc_profile(&mut encoder, p3.clone()).unwrap();
        image::ImageEncoder::write_image(
            encoder,
            &[90; 8 * 8 * 3],
            8,
            8,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
        let (srgb, _) = load("chart-srgb.png");
        let source = Source {
            bytes,
            format: ImageFormat::Tiff,
            ..srgb
        };
        assert_eq!(embedded_icc(&source), EmbeddedIcc::Profile(p3.clone()));
    }

    #[test]
    fn a_profile_over_the_limit_is_unreadable() {
        use crate::photo::decode::MAX_ICC_BYTES;
        use crate::photo::decode::tests::png_with_icc_of;
        let dir = crate::testdir::TestDir::new();
        let source = |size| {
            let path = dir.path().join(format!("{size}.png"));
            std::fs::write(&path, png_with_icc_of(size)).unwrap();
            read_source(&path).unwrap()
        };
        let limit = MAX_ICC_BYTES as usize;
        assert_eq!(embedded_icc(&source(limit + 1)), EmbeddedIcc::Unreadable);
        assert!(
            matches!(embedded_icc(&source(limit)), EmbeddedIcc::Profile(p) if p.len() == limit)
        );
    }
}
