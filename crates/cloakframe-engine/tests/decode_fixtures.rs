//! Reading photos against the 1.12.1 legacy fixtures (tests/fixtures/legacy): formats decided by
//! content, several frames found without decoding them, EXIF orientation, and the extensions the
//! scanner takes as images.

mod common;

use std::io::{self, Cursor};
use std::path::{Path, PathBuf};

use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::photo::budget::MemoryBudget;
use cloakframe_engine::photo::decode::{Decoded, ReadError, decode, exif_orientation, read_source};
use cloakframe_engine::photo::format::{ImageFormat, has_several_frames};
use cloakframe_engine::scan::{MediaKind, ScanRequest, scan};
use common::{TestDir, fixtures};
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, GenericImageView, ImageEncoder};
use serde_json::Value;

fn legacy() -> PathBuf {
    fixtures().join("legacy/tests")
}

fn calls(test: &str) -> Vec<Value> {
    let file = std::fs::read(legacy().join(format!("{test}.json"))).unwrap();
    let json: Value = serde_json::from_slice(&file).unwrap();
    json["calls"].as_array().unwrap().clone()
}

fn copy_of(call: &Value) -> PathBuf {
    legacy().join(call["file"]["copy"].as_str().unwrap())
}

fn decoded(path: &Path) -> Result<Decoded, ReadError> {
    let source = read_source(path)?;
    decode(&source, &MemoryBudget::new(1 << 30), &CancelToken::new())
}

/// 1.12.1 testImageFormatsAreDecidedByContent. The fixture numbers formats in 1.12.1's enum
/// order; `null` means the file is not an image.
#[test]
fn formats_are_decided_by_content() {
    const FORMATS: [ImageFormat; 5] = [
        ImageFormat::Jpeg,
        ImageFormat::Png,
        ImageFormat::Bmp,
        ImageFormat::Tiff,
        ImageFormat::Webp,
    ];
    let calls = calls("testImageFormatsAreDecidedByContent");
    assert_eq!(calls.len(), 9);
    for call in calls {
        let name = call["file"]["name"].as_str().unwrap();
        if call["file"]["exists"] == false {
            let missing = legacy()
                .join("testImageFormatsAreDecidedByContent")
                .join(name);
            assert!(matches!(
                read_source(&missing),
                Err(ReadError::Io(e)) if e.kind() == io::ErrorKind::NotFound
            ));
            continue;
        }
        let path = copy_of(&call);
        match call["format"].as_u64() {
            Some(i) => {
                let format = FORMATS[i as usize];
                assert_eq!(read_source(&path).unwrap().format, format, "{name}");
                decoded(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
            }
            None => assert!(
                matches!(read_source(&path), Err(ReadError::NotAnImage)),
                "{name}"
            ),
        }
    }
}

/// 1.12.1 testAnimatedImageContainersAreDetectedWithoutDecodingAllFrames: an APNG, an animated
/// WebP and a two-page BigTIFF, each only a header, are found to hold several frames and refused.
#[test]
fn several_frames_are_found_from_the_container_alone() {
    let calls = calls("testAnimatedImageContainersAreDetectedWithoutDecodingAllFrames");
    let mut formats = Vec::new();
    for call in calls {
        assert!(call["frames"].as_u64().unwrap() > 1);
        let path = copy_of(&call);
        let source = read_source(&path).unwrap();
        assert!(has_several_frames(&source.bytes, source.format), "{path:?}");
        assert!(
            matches!(decoded(&path), Err(ReadError::SeveralFrames(f)) if f == source.format),
            "{path:?}"
        );
        formats.push(source.format);
    }
    assert_eq!(
        formats,
        [ImageFormat::Png, ImageFormat::Webp, ImageFormat::Tiff]
    );
}

/// 1.12.1 testExifOrientationFallback: the orientation of each 3×2 JPEG is read from its EXIF
/// block and applied, so 5–8 come out 2×3.
#[test]
fn every_exif_orientation_is_read_and_applied() {
    let calls = calls("testExifOrientationFallback");
    let files: Vec<_> = calls
        .iter()
        .filter(|c| c["fn"] == "readExifOrientation")
        .collect();
    assert_eq!(files.len(), 9);
    for call in files {
        let path = copy_of(call);
        let want = call["orientation"].as_u64().unwrap() as u8;
        assert_eq!(exif_orientation(&std::fs::read(&path).unwrap()), want);
        let d = decoded(&path).unwrap();
        assert_eq!(d.orientation, want);
        let size = if want >= 5 { (2, 3) } else { (3, 2) };
        assert_eq!(d.image.dimensions(), size, "orientation {want}");
    }
}

/// An EXIF block (big-endian TIFF) holding only Orientation, as PNG's eXIf chunk carries it.
fn orientation_exif(orientation: u16) -> Vec<u8> {
    let mut b = b"MM\0\x2a\0\0\0\x08\0\x01".to_vec();
    b.extend([0x01, 0x12, 0, 3, 0, 0, 0, 1]);
    b.extend(orientation.to_be_bytes());
    b.extend([0, 0, 0, 0, 0, 0]);
    b
}

fn gray_png(rows: &[Vec<u8>], orientation: u16) -> Vec<u8> {
    let (w, h) = (rows[0].len() as u32, rows.len() as u32);
    let mut out = Cursor::new(Vec::new());
    let mut encoder = PngEncoder::new(&mut out);
    encoder
        .set_exif_metadata(orientation_exif(orientation))
        .unwrap();
    encoder
        .write_image(&rows.concat(), w, h, ExtendedColorType::L8)
        .unwrap();
    out.into_inner()
}

fn rows(value: &Value) -> Vec<Vec<u8>> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            r.as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u8)
                .collect()
        })
        .collect()
}

/// 1.12.1 testOrientationTransforms: the decoder moves every pixel where the EXIF orientation
/// says, compared with 1.12.1's recorded output.
#[test]
fn orientation_moves_pixels_like_1_12_1() {
    let dir = TestDir::new();
    let calls = calls("testOrientationTransforms");
    assert_eq!(calls.len(), 3);
    for call in calls {
        let orientation = call["orientation"].as_u64().unwrap() as u16;
        let path = dir.write_bytes(
            &format!("o{orientation}.png"),
            &gray_png(&rows(&call["input"]), orientation),
        );
        let d = decoded(&path).unwrap();
        assert_eq!(d.orientation as u16, orientation);
        let out = d.image.to_luma8();
        let want = rows(&call["output"]);
        assert_eq!(
            out.dimensions(),
            (want[0].len() as u32, want.len() as u32),
            "orientation {orientation}"
        );
        assert_eq!(out.into_raw(), want.concat(), "orientation {orientation}");
    }
}

/// 1.12.1 testSupportedImageExtensions: the scanner takes the five formats by extension in any
/// case and counts anything else as unsupported.
#[test]
fn the_scanner_takes_the_supported_image_extensions_in_any_case() {
    let dir = TestDir::new();
    let images = [
        "a.jpg", "b.JPEG", "c.png", "d.Bmp", "e.tif", "f.TIFF", "g.webp",
    ];
    for name in images.iter().chain(&["h.txt", "i.gif"]) {
        dir.write_bytes(name, b"x");
    }
    let result = scan(
        &ScanRequest {
            inputs: vec![dir.path().to_path_buf()],
            recursive: false,
            include_videos: false,
            output: None,
        },
        &CancelToken::new(),
    )
    .unwrap();
    let mut found: Vec<String> = result
        .files
        .iter()
        .inspect(|f| assert_eq!(f.kind, MediaKind::Image))
        .map(|f| f.relative.to_string_lossy().into_owned())
        .collect();
    found.sort();
    assert_eq!(found, images);
    assert_eq!(
        result.skipped,
        [("gif".into(), 1), ("txt".into(), 1)].into()
    );
}
