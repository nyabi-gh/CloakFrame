use std::fmt;
use std::fs::File;
use std::io::{self, Cursor, Read};
use std::path::Path;

use image::metadata::Orientation;
use image::{DynamicImage, ImageReader};

use crate::cancel::CancelToken;
use crate::detect::Rgb8Image;
use crate::identity::SourceStamp;
use crate::photo::budget::{BudgetError, MemoryBudget, MemoryPermit, estimated_bytes};
use crate::photo::format::{ImageFormat, has_several_frames, sniff};

pub const MAX_FILE_BYTES: u64 = 2 << 30;
pub const MAX_PIXELS: u64 = 512_000_000;

#[derive(Debug)]
pub enum ReadError {
    Io(io::Error),
    FileTooLarge { bytes: u64 },
    NotAnImage,
    SeveralFrames(ImageFormat),
    TooManyPixels { width: u32, height: u32 },
    Memory(BudgetError),
    Unsupported(&'static str),
    Decode(String),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read the file: {e}"),
            Self::FileTooLarge { bytes } => write!(
                f,
                "the file is {} MiB; images up to {} MiB are supported",
                bytes >> 20,
                MAX_FILE_BYTES >> 20
            ),
            Self::NotAnImage => f.write_str("the file is not a JPEG, PNG, BMP, TIFF or WebP image"),
            Self::SeveralFrames(format) => write!(
                f,
                "the {format} file holds several images (animation or pages), which is not supported"
            ),
            Self::TooManyPixels { width, height } => write!(
                f,
                "the image is {width}×{height}; images up to {} megapixels are supported",
                MAX_PIXELS / 1_000_000
            ),
            Self::Memory(e) => e.fmt(f),
            Self::Unsupported(what) => write!(f, "{what} is not supported"),
            Self::Decode(e) => write!(f, "the image could not be decoded: {e}"),
        }
    }
}

impl std::error::Error for ReadError {}

impl From<io::Error> for ReadError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// The source as it was read: every later step (decode, orientation, ICC, metadata) uses these
/// bytes, and the file itself is not opened again until the source is checked before publishing.
#[derive(Debug)]
pub struct Source {
    pub bytes: Vec<u8>,
    pub stamp: SourceStamp,
    pub format: ImageFormat,
}

pub fn read_source(path: &Path) -> Result<Source, ReadError> {
    let stamp = SourceStamp::of(path)?;
    if stamp.size > MAX_FILE_BYTES {
        return Err(ReadError::FileTooLarge { bytes: stamp.size });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(stamp.size).unwrap_or(0));
    File::open(path)?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(ReadError::FileTooLarge {
            bytes: bytes.len() as u64,
        });
    }
    let format = sniff(&bytes).ok_or(ReadError::NotAnImage)?;
    Ok(Source {
        bytes,
        stamp,
        format,
    })
}

/// Decoded pixels in display orientation. U8/U16 samples and alpha are kept as stored; floating
/// point samples are refused.
#[derive(Debug)]
pub struct Decoded {
    pub image: DynamicImage,
    /// The EXIF orientation that was applied (1 when there was none).
    pub orientation: u8,
    /// Holds the memory this image may use until the item is done with it.
    pub permit: MemoryPermit,
}

fn reader(source: &Source) -> ImageReader<Cursor<&[u8]>> {
    let format = match source.format {
        ImageFormat::Jpeg => image::ImageFormat::Jpeg,
        ImageFormat::Png => image::ImageFormat::Png,
        ImageFormat::Bmp => image::ImageFormat::Bmp,
        ImageFormat::Tiff => image::ImageFormat::Tiff,
        ImageFormat::Webp => image::ImageFormat::WebP,
    };
    let mut reader = ImageReader::with_format(Cursor::new(source.bytes.as_slice()), format);
    // The pixel limit and the memory budget below are the limits; image's default allocation
    // cap (512 MiB) would refuse supported images.
    reader.no_limits();
    reader
}

/// Width and height as stored, read from the header without decoding.
pub fn dimensions(source: &Source) -> Result<(u32, u32), ReadError> {
    if source.format == ImageFormat::Jpeg {
        let header =
            turbojpeg::read_header(&source.bytes).map_err(|e| ReadError::Decode(e.to_string()))?;
        let side = |v: usize| u32::try_from(v).map_err(|_| ReadError::Decode("size".into()));
        return Ok((side(header.width)?, side(header.height)?));
    }
    reader(source)
        .into_dimensions()
        .map_err(|e| ReadError::Decode(e.to_string()))
}

pub fn decode(
    source: &Source,
    budget: &MemoryBudget,
    cancel: &CancelToken,
) -> Result<Decoded, ReadError> {
    if has_several_frames(&source.bytes, source.format) {
        return Err(ReadError::SeveralFrames(source.format));
    }
    let (width, height) = dimensions(source)?;
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_PIXELS {
        return Err(ReadError::TooManyPixels { width, height });
    }
    let permit = budget
        .acquire(estimated_bytes(pixels, source.bytes.len() as u64), cancel)
        .map_err(ReadError::Memory)?;
    let mut image = if source.format == ImageFormat::Jpeg {
        decode_jpeg(&source.bytes)?
    } else {
        reader(source)
            .decode()
            .map_err(|e| ReadError::Decode(e.to_string()))?
    };
    if matches!(
        image,
        DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_)
    ) {
        return Err(ReadError::Unsupported(
            "an image with floating-point samples",
        ));
    }
    let orientation = exif_orientation(&source.bytes);
    if let Some(o) = Orientation::from_exif(orientation) {
        image.apply_orientation(o);
    }
    Ok(Decoded {
        image,
        orientation,
        permit,
    })
}

/// libjpeg-turbo rather than image's decoder, which silently decodes non-interleaved 4:2:0
/// files to wrong pixels (docs/M2_ENCODE_SPIKE.md) and differs from the OpenCV decode 1.12.1 used.
fn decode_jpeg(bytes: &[u8]) -> Result<DynamicImage, ReadError> {
    let failed = |e: turbojpeg::Error| ReadError::Decode(e.to_string());
    let header = turbojpeg::read_header(bytes).map_err(failed)?;
    let (format, gray) = match header.colorspace {
        turbojpeg::Colorspace::Gray => (turbojpeg::PixelFormat::GRAY, true),
        turbojpeg::Colorspace::CMYK | turbojpeg::Colorspace::YCCK => {
            return Err(ReadError::Unsupported("a CMYK JPEG"));
        }
        _ => (turbojpeg::PixelFormat::RGB, false),
    };
    let decoded = turbojpeg::decompress(bytes, format).map_err(failed)?;
    let (w, h) = (decoded.width as u32, decoded.height as u32);
    let image = if gray {
        image::GrayImage::from_raw(w, h, decoded.pixels).map(DynamicImage::ImageLuma8)
    } else {
        image::RgbImage::from_raw(w, h, decoded.pixels).map(DynamicImage::ImageRgb8)
    };
    image.ok_or_else(|| ReadError::Decode("unexpected buffer size".into()))
}

/// 1–8 from the EXIF block in any of the containers, 1 when there is none or it is invalid.
pub fn exif_orientation(bytes: &[u8]) -> u8 {
    exif::Reader::new()
        .read_from_container(&mut Cursor::new(bytes))
        .ok()
        .and_then(|e| {
            e.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .and_then(|f| f.value.get_uint(0))
        })
        .and_then(|v| u8::try_from(v).ok())
        .filter(|v| (1..=8).contains(v))
        .unwrap_or(1)
}

/// The 8-bit RGB copy detectors read, as 1.12.1 made it: 16-bit samples round to 8 bits, gray is
/// repeated into the three channels and alpha is dropped (not composited).
pub fn detection_rgb8(image: &DynamicImage) -> Rgb8Image {
    let rgb = image.to_rgb8();
    Rgb8Image {
        width: rgb.width(),
        height: rgb.height(),
        data: rgb.into_raw(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;
    use image::{GenericImageView, ImageEncoder, Rgb, RgbImage};

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/decode")
            .join(name)
    }

    fn budget() -> MemoryBudget {
        MemoryBudget::new(1 << 30)
    }

    fn solid(w: u32, h: u32) -> RgbImage {
        RgbImage::from_pixel(w, h, Rgb([10, 20, 30]))
    }

    fn encode(image: &RgbImage, format: image::ImageFormat) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    fn jpeg(image: &RgbImage) -> Vec<u8> {
        let mut c = turbojpeg::Compressor::new().unwrap();
        c.set_quality(92).unwrap();
        c.compress_to_vec(turbojpeg::Image {
            pixels: image.as_raw().as_slice(),
            width: image.width() as usize,
            pitch: image.width() as usize * 3,
            height: image.height() as usize,
            format: turbojpeg::PixelFormat::RGB,
        })
        .unwrap()
    }

    /// A JPEG with an APP1 EXIF block holding only IFD0 Orientation.
    fn jpeg_with_orientation(image: &RgbImage, orientation: u16) -> Vec<u8> {
        let mut tiff = b"II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0".to_vec();
        tiff.extend(orientation.to_le_bytes());
        tiff.extend([0, 0, 0, 0, 0, 0]);
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend(tiff);
        let plain = jpeg(image);
        let mut out = plain[..2].to_vec();
        out.extend([0xFF, 0xE1]);
        out.extend(((app1.len() + 2) as u16).to_be_bytes());
        out.extend(app1);
        out.extend(&plain[2..]);
        out
    }

    fn source(dir: &TestDir, name: &str, bytes: &[u8]) -> Result<Source, ReadError> {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        read_source(&path)
    }

    #[test]
    fn formats_are_decided_by_content_not_name() {
        let dir = TestDir::new();
        let pixels = solid(8, 8);
        let cases = [
            (jpeg(&pixels), ImageFormat::Jpeg),
            (encode(&pixels, image::ImageFormat::Png), ImageFormat::Png),
            (encode(&pixels, image::ImageFormat::Bmp), ImageFormat::Bmp),
            (encode(&pixels, image::ImageFormat::Tiff), ImageFormat::Tiff),
            (encode(&pixels, image::ImageFormat::WebP), ImageFormat::Webp),
        ];
        for (bytes, format) in cases {
            let s = source(&dir, "image.bin", &bytes).unwrap();
            assert_eq!(s.format, format);
            let decoded = decode(&s, &budget(), &CancelToken::new()).unwrap();
            assert_eq!(decoded.image.dimensions(), (8, 8), "{format}");
        }
        let ppm = b"P6\n1 1\n255\n\x01\x02\x03";
        assert!(matches!(
            source(&dir, "netpbm.jpg", ppm),
            Err(ReadError::NotAnImage)
        ));
        assert!(matches!(
            source(&dir, "short.jpg", b"\xFF\xD8"),
            Err(ReadError::NotAnImage)
        ));
        assert!(matches!(
            source(&dir, "riff.webp", b"RIFF\0\0\0\0WAVEfmt "),
            Err(ReadError::NotAnImage)
        ));
        assert!(matches!(
            read_source(&dir.path().join("missing.png")),
            Err(ReadError::Io(e)) if e.kind() == io::ErrorKind::NotFound
        ));
        let renamed = source(
            &dir,
            "renamed.jpg",
            &encode(&pixels, image::ImageFormat::Png),
        )
        .unwrap();
        assert_eq!(renamed.format, ImageFormat::Png);
    }

    // The case image's decoder (zune-jpeg 0.5.15) gets wrong without an error.
    #[test]
    fn non_interleaved_420_jpeg_decodes_like_libjpeg() {
        for name in ["noninterleaved-420", "interleaved-420"] {
            let s = read_source(&fixture(&format!("{name}.jpg"))).unwrap();
            let decoded = decode(&s, &budget(), &CancelToken::new()).unwrap();
            let expected = image::open(fixture(&format!("{name}.djpeg.png")))
                .unwrap()
                .to_rgb8();
            assert_eq!(decoded.image.to_rgb8(), expected, "{name}");
        }
    }

    #[test]
    fn reads_every_exif_orientation_and_applies_it() {
        let dir = TestDir::new();
        let base = RgbImage::from_fn(3, 2, |x, y| Rgb([(y * 10 + x) as u8 * 20, 0, 0]));
        for o in 1..=8u16 {
            let bytes = jpeg_with_orientation(&base, o);
            assert_eq!(exif_orientation(&bytes), o as u8);
            let s = source(&dir, &format!("o{o}.jpg"), &bytes).unwrap();
            let decoded = decode(&s, &budget(), &CancelToken::new()).unwrap();
            let expected = if o >= 5 { (2, 3) } else { (3, 2) };
            assert_eq!(decoded.image.dimensions(), expected, "orientation {o}");
            assert_eq!(decoded.orientation, o as u8);
        }
        assert_eq!(exif_orientation(&jpeg_with_orientation(&base, 9)), 1);
        assert_eq!(exif_orientation(&jpeg(&base)), 1);
    }

    #[test]
    fn orientation_transforms_move_pixels_as_exif_defines() {
        let base = image::GrayImage::from_fn(3, 2, |x, y| image::Luma([(y * 10 + x) as u8]));
        let oriented = |o: u8| {
            let mut i = DynamicImage::ImageLuma8(base.clone());
            i.apply_orientation(Orientation::from_exif(o).unwrap());
            i.to_luma8()
        };
        assert_eq!(oriented(1), base);
        let rotated = oriented(6);
        assert_eq!(rotated.dimensions(), (2, 3));
        assert_eq!(rotated.get_pixel(0, 0), base.get_pixel(0, 1));
        let mirrored = oriented(2);
        assert_eq!(mirrored.dimensions(), (3, 2));
        assert_eq!(mirrored.get_pixel(0, 0), base.get_pixel(2, 0));
    }

    #[test]
    fn a_large_jpeg_with_trailing_bytes_still_decodes() {
        let dir = TestDir::new();
        let mut bytes = jpeg(&solid(64, 64));
        bytes.resize(31 << 20, 0);
        let s = source(&dir, "large.jpg", &bytes).unwrap();
        let decoded = decode(&s, &budget(), &CancelToken::new()).unwrap();
        assert_eq!(decoded.image.dimensions(), (64, 64));
    }

    /// A PNG of only the signature, IHDR declaring the size, and an empty IDAT: the size is read
    /// from the header without decoding.
    fn png_declaring(width: u32, height: u32) -> Vec<u8> {
        let mut ihdr = b"IHDR".to_vec();
        ihdr.extend(width.to_be_bytes());
        ihdr.extend(height.to_be_bytes());
        ihdr.extend([8, 2, 0, 0, 0]);
        let chunk = |out: &mut Vec<u8>, body: &[u8]| {
            out.extend(((body.len() - 4) as u32).to_be_bytes());
            out.extend(body);
            out.extend(crc32(body).to_be_bytes());
        };
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        chunk(&mut out, &ihdr);
        chunk(&mut out, b"IDAT");
        chunk(&mut out, b"IEND");
        out
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in bytes {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    #[test]
    fn refuses_images_past_the_pixel_limit_or_the_memory_budget_before_decoding() {
        let dir = TestDir::new();
        let huge = source(&dir, "huge.png", &png_declaring(30_000, 20_000)).unwrap();
        assert_eq!(dimensions(&huge).unwrap(), (30_000, 20_000));
        assert!(matches!(
            decode(&huge, &budget(), &CancelToken::new()),
            Err(ReadError::TooManyPixels {
                width: 30_000,
                height: 20_000
            })
        ));
        let big = source(&dir, "big.png", &png_declaring(10_000, 10_000)).unwrap();
        assert!(matches!(
            decode(&big, &MemoryBudget::new(1 << 30), &CancelToken::new()),
            Err(ReadError::Memory(BudgetError::TooLarge { .. }))
        ));
    }

    #[test]
    fn refuses_files_with_several_frames() {
        let dir = TestDir::new();
        let pages = crate::photo::format::tests::tiff_pages(true, false, 2);
        let s = source(&dir, "pages.tiff", &pages).unwrap();
        assert!(matches!(
            decode(&s, &budget(), &CancelToken::new()),
            Err(ReadError::SeveralFrames(ImageFormat::Tiff))
        ));
    }

    #[test]
    fn keeps_sample_depth_and_alpha_and_makes_the_1121_detection_copy() {
        let dir = TestDir::new();
        let rgba16 = image::ImageBuffer::<image::Rgba<u16>, _>::from_fn(2, 1, |x, _| {
            image::Rgba([if x == 0 { 65535 } else { 128 }, 32896, 0, 1000])
        });
        let native: Vec<u8> = rgba16
            .as_raw()
            .iter()
            .flat_map(|s| s.to_ne_bytes())
            .collect();
        let mut png = Cursor::new(Vec::new());
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&native, 2, 1, image::ExtendedColorType::Rgba16)
            .unwrap();
        let s = source(&dir, "deep.png", png.get_ref()).unwrap();
        let decoded = decode(&s, &budget(), &CancelToken::new()).unwrap();
        assert!(matches!(decoded.image, DynamicImage::ImageRgba16(_)));
        let detect = detection_rgb8(&decoded.image);
        assert_eq!(detect.data, [255, 128, 0, 0, 128, 0]);

        let gray = image::GrayImage::from_pixel(1, 1, image::Luma([77]));
        assert_eq!(
            detection_rgb8(&DynamicImage::ImageLuma8(gray)).data,
            [77, 77, 77]
        );
    }

    #[test]
    fn refuses_a_file_over_two_gigabytes_without_reading_it() {
        let dir = TestDir::new();
        let path = dir.path().join("sparse.jpg");
        let f = File::create(&path).unwrap();
        f.set_len(MAX_FILE_BYTES + 1).unwrap();
        assert!(matches!(
            read_source(&path),
            Err(ReadError::FileTooLarge { .. })
        ));
    }
}
