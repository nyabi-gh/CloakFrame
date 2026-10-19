//! Writes the processed photo with the D6 defaults: JPEG q92 with the source's chroma
//! subsampling, lossy WebP q90 for a lossy source, and lossless PNG, WebP and TIFF (LZW)
//! otherwise. Metadata follows [`MetadataPolicy`] and the color outcome (D2); nothing else from
//! the source reaches the output (D9).

use std::borrow::Cow;
use std::fmt;
use std::io::Cursor;

use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use image::{DynamicImage, ImageEncoder};
use tiff::encoder::colortype::{Gray8, Gray16, RGB8, RGB16, RGBA8, RGBA16};
use tiff::encoder::{Compression, Predictor, TiffEncoder};

use super::color::{ColorOutcome, Standard};
use super::decode::Source;
use super::format::ImageFormat;
use super::metadata::{
    self, Container, ContainerError, MetadataPolicy, Output, exif_block, jpeg_with, tiff_with,
    webp_with,
};

pub const JPEG_QUALITY: i32 = 92;
pub const WEBP_LOSSY_QUALITY: f32 = 90.0;

#[derive(Debug)]
pub enum EncodeError {
    Codec(String),
    Metadata(ContainerError),
    /// A profile other than sRGB for a BMP. The processor converts such pixels to sRGB first
    /// ([`super::color::srgb_for_bmp`]).
    ProfileNotEmbeddable,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(reason) => write!(f, "could not encode the photo: {reason}"),
            Self::Metadata(e) => e.fmt(f),
            Self::ProfileNotEmbeddable => f.write_str("a BMP cannot carry this color profile"),
        }
    }
}

impl std::error::Error for EncodeError {}

impl From<ContainerError> for EncodeError {
    fn from(e: ContainerError) -> Self {
        Self::Metadata(e)
    }
}

fn codec(e: impl fmt::Display) -> EncodeError {
    EncodeError::Codec(e.to_string())
}

/// The encoded file for `image`, which came from `source` and went through the color policy, in
/// `format`. That is the source's format unless the file is named for another one: 1.12.1 wrote a
/// PNG named `.jpg` as a JPEG.
pub fn encode(
    source: &Source,
    image: &DynamicImage,
    color: ColorOutcome,
    policy: MetadataPolicy,
    format: ImageFormat,
) -> Result<Vec<u8>, EncodeError> {
    let standard = color.embed();
    let icc = standard.map(Standard::profile_bytes);
    let container = match format {
        ImageFormat::Jpeg => Container::Jpeg,
        ImageFormat::Png => Container::Png,
        ImageFormat::Webp => Container::Webp,
        ImageFormat::Tiff => Container::Tiff,
        ImageFormat::Bmp => {
            if !matches!(standard, None | Some(Standard::Srgb)) {
                return Err(EncodeError::ProfileNotEmbeddable);
            }
            let mut out = Cursor::new(Vec::new());
            eight_bit(image)
                .write_to(&mut out, image::ImageFormat::Bmp)
                .map_err(codec)?;
            return Ok(out.into_inner());
        }
    };
    let kept = metadata::keep(&source.bytes, policy);
    let output = Output {
        width: image.width(),
        height: image.height(),
        container,
        srgb: matches!(standard, None | Some(Standard::Srgb)),
    };
    let exif = (!kept.is_empty()).then(|| exif_block(&kept, output));
    match container {
        Container::Jpeg => Ok(jpeg_with(&jpeg(source, image)?, exif.as_deref(), icc)?),
        Container::Png => {
            let mut out = Vec::new();
            let mut encoder = PngEncoder::new(&mut out);
            if let Some(profile) = icc {
                encoder.set_icc_profile(profile.to_vec()).map_err(codec)?;
            }
            if let Some(block) = exif {
                encoder.set_exif_metadata(block).map_err(codec)?;
            }
            encoder
                .write_image(
                    image.as_bytes(),
                    image.width(),
                    image.height(),
                    image.color().into(),
                )
                .map_err(codec)?;
            Ok(out)
        }
        Container::Webp => {
            let plain = if source.format == ImageFormat::Webp && is_lossy_webp(&source.bytes) {
                lossy_webp(image)?
            } else {
                lossless_webp(image)?
            };
            Ok(webp_with(&plain, exif.as_deref(), icc)?)
        }
        Container::Tiff => Ok(tiff_with(&tiff(image)?, &kept, icc, output)?),
    }
}

/// 16-bit samples rounded to 8 bits, for encoders without 16-bit support.
fn eight_bit(image: &DynamicImage) -> Cow<'_, DynamicImage> {
    match image {
        DynamicImage::ImageLuma16(_) => Cow::Owned(DynamicImage::ImageLuma8(image.to_luma8())),
        DynamicImage::ImageLumaA16(_) => {
            Cow::Owned(DynamicImage::ImageLumaA8(image.to_luma_alpha8()))
        }
        DynamicImage::ImageRgb16(_) => Cow::Owned(DynamicImage::ImageRgb8(image.to_rgb8())),
        DynamicImage::ImageRgba16(_) => Cow::Owned(DynamicImage::ImageRgba8(image.to_rgba8())),
        _ => Cow::Borrowed(image),
    }
}

/// q92 with the source's subsampling (D6); 1.12.1 always wrote q100 4:4:4. A source that is not
/// a JPEG, or whose subsampling libjpeg-turbo cannot write, gets full chroma. JPEG has no alpha, which is dropped as 1.12.1 did, and no
/// 16-bit samples, which are scaled to 8 bits.
fn jpeg(source: &Source, image: &DynamicImage) -> Result<Vec<u8>, EncodeError> {
    let source_subsamp = if source.format == ImageFormat::Jpeg {
        turbojpeg::read_header(&source.bytes)
            .map_err(codec)?
            .subsamp
    } else {
        turbojpeg::Subsamp::None
    };
    let mut compressor = turbojpeg::Compressor::new().map_err(codec)?;
    compressor.set_quality(JPEG_QUALITY).map_err(codec)?;
    compressor.set_optimize(true).map_err(codec)?;
    let (format, subsamp, pixels): (_, _, Cow<'_, [u8]>) = match image {
        DynamicImage::ImageLuma8(gray) => (
            turbojpeg::PixelFormat::GRAY,
            turbojpeg::Subsamp::Gray,
            Cow::Borrowed(gray.as_raw()),
        ),
        _ if !image.color().has_color() => (
            turbojpeg::PixelFormat::GRAY,
            turbojpeg::Subsamp::Gray,
            Cow::Owned(image.to_luma8().into_raw()),
        ),
        _ => {
            let subsamp = match source_subsamp {
                // Unknown: a combination libjpeg-turbo cannot write, such as 2x1,1x2,1x1.
                turbojpeg::Subsamp::Gray | turbojpeg::Subsamp::Unknown => turbojpeg::Subsamp::None,
                other => other,
            };
            let rgb = match image {
                DynamicImage::ImageRgb8(rgb) => Cow::Borrowed(rgb.as_raw().as_slice()),
                _ => Cow::Owned(image.to_rgb8().into_raw()),
            };
            (turbojpeg::PixelFormat::RGB, subsamp, rgb)
        }
    };
    compressor.set_subsamp(subsamp).map_err(codec)?;
    let channels = format.size();
    compressor
        .compress_to_vec(turbojpeg::Image {
            pixels: &*pixels,
            width: image.width() as usize,
            pitch: image.width() as usize * channels,
            height: image.height() as usize,
            format,
        })
        .map_err(codec)
}

/// Whether the source WebP's image is VP8 (lossy), alone or with an alpha chunk.
fn is_lossy_webp(bytes: &[u8]) -> bool {
    let mut at = 12;
    while let (Some(id), Some(len)) = (
        bytes.get(at..at + 4),
        bytes
            .get(at + 4..at + 8)
            .map(|l| u32::from_le_bytes(l.try_into().expect("4 bytes")) as usize),
    ) {
        match id {
            b"VP8 " => return true,
            b"VP8L" => return false,
            _ => {
                at = at
                    .saturating_add(8)
                    .saturating_add(len)
                    .saturating_add(len & 1)
            }
        }
    }
    false
}

fn lossless_webp(image: &DynamicImage) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::new();
    let (w, h) = (image.width(), image.height());
    let encoder = WebPEncoder::new_lossless(&mut out);
    if image.color().has_alpha() {
        let rgba = image.to_rgba8();
        encoder.write_image(rgba.as_raw(), w, h, image::ExtendedColorType::Rgba8)
    } else {
        let rgb = image.to_rgb8();
        encoder.write_image(rgb.as_raw(), w, h, image::ExtendedColorType::Rgb8)
    }
    .map_err(codec)?;
    Ok(out)
}

/// libwebp at q90; alpha, if any, is compressed losslessly (libwebp's default).
#[allow(unsafe_code)]
fn lossy_webp(image: &DynamicImage) -> Result<Vec<u8>, EncodeError> {
    let (w, h) = (image.width(), image.height());
    let dimension = |v: u32| i32::try_from(v).map_err(|_| codec("image too large for WebP"));
    let (wi, hi) = (dimension(w)?, dimension(h)?);
    let alpha = image.color().has_alpha();
    let pixels = if alpha {
        image.to_rgba8().into_raw()
    } else {
        image.to_rgb8().into_raw()
    };
    let stride = wi * if alpha { 4 } else { 3 };
    let mut ptr = std::ptr::null_mut();
    // SAFETY: `pixels` is tightly packed RGB8 or RGBA8 of w x h pixels with `stride` bytes per
    // row. libwebp allocates the output, which is copied out and freed with WebPFree.
    unsafe {
        let len = if alpha {
            libwebp_sys::WebPEncodeRGBA(
                pixels.as_ptr(),
                wi,
                hi,
                stride,
                WEBP_LOSSY_QUALITY,
                &mut ptr,
            )
        } else {
            libwebp_sys::WebPEncodeRGB(
                pixels.as_ptr(),
                wi,
                hi,
                stride,
                WEBP_LOSSY_QUALITY,
                &mut ptr,
            )
        };
        if len == 0 || ptr.is_null() {
            return Err(codec("libwebp could not encode the image"));
        }
        let bytes = std::slice::from_raw_parts(ptr, len).to_vec();
        libwebp_sys::WebPFree(ptr.cast());
        Ok(bytes)
    }
}

/// LZW with horizontal differencing: lossless and in TIFF 6.0, which Deflate is not (exiftool
/// flags it). Gray with alpha has no TIFF color type in the encoder
/// and is written as RGBA.
fn tiff(image: &DynamicImage) -> Result<Vec<u8>, EncodeError> {
    let mut out = Cursor::new(Vec::new());
    let mut encoder = TiffEncoder::new(&mut out)
        .map_err(codec)?
        .with_compression(Compression::Lzw)
        .with_predictor(Predictor::Horizontal);
    let (w, h) = (image.width(), image.height());
    match image {
        DynamicImage::ImageLuma8(i) => encoder.write_image::<Gray8>(w, h, i),
        DynamicImage::ImageLuma16(i) => encoder.write_image::<Gray16>(w, h, i),
        DynamicImage::ImageRgb8(i) => encoder.write_image::<RGB8>(w, h, i),
        DynamicImage::ImageRgb16(i) => encoder.write_image::<RGB16>(w, h, i),
        DynamicImage::ImageRgba8(i) => encoder.write_image::<RGBA8>(w, h, i),
        DynamicImage::ImageRgba16(i) => encoder.write_image::<RGBA16>(w, h, i),
        DynamicImage::ImageLumaA8(_) => encoder.write_image::<RGBA8>(w, h, &image.to_rgba8()),
        DynamicImage::ImageLumaA16(_) => encoder.write_image::<RGBA16>(w, h, &image.to_rgba16()),
        _ => return Err(codec("only 8- and 16-bit images are written")),
    }
    .map_err(codec)?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel::CancelToken;
    use crate::photo::budget::MemoryBudget;
    use crate::photo::color::{EmbeddedIcc, embedded_icc};
    use crate::photo::decode::{decode, read_source};
    use crate::test_support::fixtures;
    use exif::{In, Tag};

    fn fixture(path: &str) -> Source {
        read_source(&fixtures().join(path)).unwrap()
    }

    /// rich-exif.jpg (camera, owner, GPS, XMP, IPTC, comment and "Private" text everywhere), the
    /// metadata source of every output.
    fn rich() -> (Source, DynamicImage) {
        let source = fixture("metadata/rich-exif.jpg");
        let image = decode(&source, &MemoryBudget::new(1 << 30), &CancelToken::new())
            .unwrap()
            .image;
        (source, image)
    }

    fn reread(bytes: Vec<u8>, format: ImageFormat) -> Source {
        Source {
            bytes,
            format,
            ..fixture("metadata/rich-exif.jpg")
        }
    }

    fn exif_of(bytes: &[u8]) -> Option<exif::Exif> {
        exif::Reader::new()
            .read_from_container(&mut Cursor::new(bytes))
            .ok()
    }

    fn text(exif: &exif::Exif, tag: Tag) -> Option<String> {
        exif.get_field(tag, In::PRIMARY)
            .map(|f| f.display_value().to_string())
    }

    /// Decodes the output's header with the same libraries the engine reads with.
    fn dimensions(out: &[u8], format: ImageFormat) -> (u32, u32) {
        let source = reread(out.to_vec(), format);
        crate::photo::decode::dimensions(&source).unwrap()
    }

    /// Camera, exposure or GPS tags. A TIFF's own image tags (IFD0) are not metadata.
    fn has_photo_tags(exif: &exif::Exif) -> bool {
        exif.fields().any(|f| {
            f.tag.context() != exif::Context::Tiff
                || [Tag::Make, Tag::Model, Tag::DateTime, Tag::Artist].contains(&f.tag)
        })
    }

    const FORMATS: [ImageFormat; 4] = [
        ImageFormat::Jpeg,
        ImageFormat::Png,
        ImageFormat::Webp,
        ImageFormat::Tiff,
    ];

    #[test]
    fn every_container_carries_the_allowlist_and_the_profile() {
        let p3 = ColorOutcome::Kept(Standard::DisplayP3);
        for format in FORMATS {
            for gps in [false, true] {
                let (source, image) = rich();
                let policy = MetadataPolicy::Allowlist { gps };
                let out = encode(&source, &image, p3, policy, format).unwrap();
                assert_eq!(dimensions(&out, format), (image.width(), image.height()));

                let exif = exif_of(&out).unwrap_or_else(|| panic!("{format:?} has no EXIF"));
                assert_eq!(text(&exif, Tag::Make).as_deref(), Some("\"CloakCam\""));
                assert_eq!(
                    text(&exif, Tag::LensModel).as_deref(),
                    Some("\"CL 50mm F1.8\"")
                );
                let orientation = exif.get_field(Tag::Orientation, In::PRIMARY).unwrap();
                assert_eq!(orientation.value.get_uint(0), Some(1));
                let color_space = exif.get_field(Tag::ColorSpace, In::PRIMARY).unwrap();
                assert_eq!(color_space.value.get_uint(0), Some(0xFFFF), "{format:?}");
                for private in [
                    Tag::Artist,
                    Tag::Software,
                    Tag::UserComment,
                    Tag::BodySerialNumber,
                ] {
                    assert!(exif.get_field(private, In::PRIMARY).is_none(), "{format:?}");
                }
                assert_eq!(
                    exif.get_field(Tag::GPSLatitudeRef, In::PRIMARY).is_some(),
                    gps,
                    "{format:?}"
                );
                assert!(!out.windows(7).any(|w| w == b"Private"), "{format:?}");

                let icc = embedded_icc(&reread(out, format));
                let want = Standard::DisplayP3.profile_bytes().to_vec();
                assert_eq!(icc, EmbeddedIcc::Profile(want), "{format:?}");
            }
        }
    }

    #[test]
    fn lossy_webp_gets_the_same_chunks() {
        let (source, image) = rich();
        let kept = metadata::keep(&source.bytes, MetadataPolicy::Allowlist { gps: false });
        let output = Output {
            width: image.width(),
            height: image.height(),
            container: Container::Webp,
            srgb: true,
        };
        let icc = Standard::Srgb.profile_bytes();
        let out = webp_with(
            &lossy_webp(&image).unwrap(),
            Some(&exif_block(&kept, output)),
            Some(icc),
        )
        .unwrap();
        assert!(is_lossy_webp(&out));
        assert!(image::load_from_memory(&out).is_ok());
        let exif = exif_of(&out).unwrap();
        assert_eq!(text(&exif, Tag::Model).as_deref(), Some("\"CF-1\""));
        let back = embedded_icc(&reread(out, ImageFormat::Webp));
        assert_eq!(back, EmbeddedIcc::Profile(icc.to_vec()));
    }

    /// 1.12.1 testDefaultOutputCarriesNoMetadata: the default output has none of these markers.
    /// ICC markers are allowed again since D2 but this source has no profile.
    #[test]
    fn the_default_output_carries_no_metadata() {
        const MARKERS: [&[u8]; 13] = [
            b"Private",
            b"Exif",
            b"http://ns.adobe.com/xap",
            b"adobe:ns:meta",
            b"ICC_PROFILE",
            b"acsp",
            b"iCCP",
            b"eXIf",
            b"tEXt",
            b"iTXt",
            b"zTXt",
            b"XMP ",
            b"ICCP",
        ];
        for format in FORMATS {
            let (source, image) = rich();
            let out = encode(
                &source,
                &image,
                ColorOutcome::Untagged,
                MetadataPolicy::default(),
                format,
            )
            .unwrap();
            for marker in MARKERS {
                assert!(
                    !out.windows(marker.len()).any(|w| w == marker),
                    "{format:?} has {}",
                    String::from_utf8_lossy(marker)
                );
            }
            assert!(
                !exif_of(&out).is_some_and(|e| has_photo_tags(&e)),
                "{format:?}"
            );
        }
    }

    /// Outputs named for another format than the source's: full chroma from a source that is not
    /// a JPEG, and 16-bit or alpha samples reduced where the format has none.
    #[test]
    fn writes_the_format_the_output_is_named_for() {
        let rgba16 = DynamicImage::ImageRgba16(image::ImageBuffer::from_fn(9, 7, |x, y| {
            image::Rgba([x as u16 * 7000, y as u16 * 9000, 30_000, 40_000])
        }));
        let mut png = Cursor::new(Vec::new());
        rgba16.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let source = reread(png.into_inner(), ImageFormat::Png);
        for format in [ImageFormat::Jpeg, ImageFormat::Bmp, ImageFormat::Webp] {
            let out = encode(
                &source,
                &rgba16,
                ColorOutcome::Untagged,
                MetadataPolicy::default(),
                format,
            )
            .unwrap();
            assert_eq!(crate::photo::format::sniff(&out), Some(format));
            assert_eq!(dimensions(&out, format), (9, 7), "{format:?}");
        }
        let jpeg = encode(
            &source,
            &rgba16,
            ColorOutcome::Untagged,
            MetadataPolicy::default(),
            ImageFormat::Jpeg,
        )
        .unwrap();
        let header = turbojpeg::read_header(&jpeg).unwrap();
        assert_eq!(header.subsamp, turbojpeg::Subsamp::None);
        let webp = encode(
            &source,
            &rgba16,
            ColorOutcome::Untagged,
            MetadataPolicy::default(),
            ImageFormat::Webp,
        )
        .unwrap();
        assert!(!is_lossy_webp(&webp));
    }

    /// libjpeg-turbo writes only the standard subsampling combinations; a source with another one
    /// (`cjpeg -sample 2x1,1x2,1x1`) gets full chroma, as every output did in 1.12.1.
    #[test]
    fn a_source_with_unusual_subsampling_gets_full_chroma() {
        let source = fixture("decode/mixed-sampling.jpg");
        assert_eq!(
            turbojpeg::read_header(&source.bytes).unwrap().subsamp,
            turbojpeg::Subsamp::Unknown
        );
        let image = decode(&source, &MemoryBudget::new(1 << 30), &CancelToken::new())
            .unwrap()
            .image;
        let out = encode(
            &source,
            &image,
            ColorOutcome::Untagged,
            MetadataPolicy::default(),
            ImageFormat::Jpeg,
        )
        .unwrap();
        assert_eq!(
            turbojpeg::read_header(&out).unwrap().subsamp,
            turbojpeg::Subsamp::None
        );
    }
}
