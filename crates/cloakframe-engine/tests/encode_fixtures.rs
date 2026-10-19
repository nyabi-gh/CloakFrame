//! Output encoding and metadata against the fixtures: D6 (tests/fixtures/defects/D6), the
//! allowlist (tests/fixtures/metadata), D9 and the 1.12.1 metadata tests.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::photo::budget::MemoryBudget;
use cloakframe_engine::photo::decode::{Source, decode, read_source};
use cloakframe_engine::photo::metadata::MetadataPolicy;
use cloakframe_engine::photo::{color, encode};
use exif::{In, Tag, Value};
use image::DynamicImage;
use serde::Deserialize;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

/// The photo pipeline without masks: read, decode, color policy, encode.
fn export(path: &Path, policy: MetadataPolicy) -> (Source, DynamicImage, Vec<u8>) {
    let source = read_source(path).unwrap();
    let mut image = decode(&source, &MemoryBudget::new(1 << 30), &CancelToken::new())
        .unwrap()
        .image;
    let outcome = color::apply(&source, &mut image).unwrap();
    let out = encode::encode(&source, &image, outcome, policy, source.format).unwrap();
    (source, image, out)
}

fn exif_of(bytes: &[u8]) -> Option<exif::Exif> {
    exif::Reader::new()
        .read_from_container(&mut Cursor::new(bytes))
        .ok()
}

/// Quantization table 0 in zigzag order.
fn luma_table(jpeg: &[u8]) -> Vec<u16> {
    let mut at = 2;
    while at + 4 <= jpeg.len() {
        let marker = jpeg[at + 1];
        let len = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
        if marker == 0xDB {
            let mut p = at + 4;
            while p < at + 2 + len {
                let (precision, id) = (jpeg[p] >> 4, jpeg[p] & 0x0F);
                let size = if precision == 0 { 64 } else { 128 };
                if id == 0 {
                    return (0..64)
                        .map(|i| {
                            if precision == 0 {
                                u16::from(jpeg[p + 1 + i])
                            } else {
                                u16::from_be_bytes([jpeg[p + 1 + 2 * i], jpeg[p + 2 + 2 * i]])
                            }
                        })
                        .collect();
                }
                p += 1 + size;
            }
        }
        at += 2 + len;
    }
    panic!("no luminance table");
}

/// libjpeg's standard luminance table scaled to `quality`, in zigzag order.
fn ijg_luma_table(quality: u32) -> Vec<u16> {
    const STD: [u32; 64] = [
        16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69,
        56, 14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81,
        104, 113, 92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
    ];
    const ZIGZAG: [usize; 64] = [
        0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27,
        20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51,
        58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
    ];
    let scale = if quality < 50 {
        5000 / quality
    } else {
        200 - 2 * quality
    };
    ZIGZAG
        .iter()
        .map(|&n| ((STD[n] * scale + 50) / 100).clamp(1, 255) as u16)
        .collect()
}

/// D6. 1.12.1 testEncodeParams asked for q100 4:4:4 instead.
#[test]
fn d6_jpeg_is_q92_with_the_source_subsampling() {
    for (name, subsamp) in [
        ("q90-420.jpg", turbojpeg::Subsamp::Sub2x2),
        ("q85-422.jpg", turbojpeg::Subsamp::Sub2x1),
        ("q95-444.jpg", turbojpeg::Subsamp::None),
    ] {
        let (_, _, out) = export(
            &fixtures().join("defects/D6").join(name),
            MetadataPolicy::Strip,
        );
        assert_eq!(
            turbojpeg::read_header(&out).unwrap().subsamp,
            subsamp,
            "{name}"
        );
        assert_eq!(luma_table(&out), ijg_luma_table(92), "{name}");
    }
}

fn first_webp_image_chunk(webp: &[u8]) -> [u8; 4] {
    let mut at = 12;
    loop {
        let id: [u8; 4] = webp[at..at + 4].try_into().unwrap();
        if matches!(&id, b"VP8 " | b"VP8L") {
            return id;
        }
        let len = u32::from_le_bytes(webp[at + 4..at + 8].try_into().unwrap()) as usize;
        at += 8 + len + (len & 1);
    }
}

/// libwebp's simple encoder at `quality`, as `cwebp -q` with no other option.
#[allow(unsafe_code)]
fn libwebp_rgb(image: &image::RgbImage, quality: f32) -> Vec<u8> {
    let (w, h) = (image.width() as i32, image.height() as i32);
    let mut ptr = std::ptr::null_mut();
    // SAFETY: the buffer is tightly packed RGB8 of w x h; libwebp allocates the output, which is
    // copied and freed with WebPFree.
    unsafe {
        let len = libwebp_sys::WebPEncodeRGB(image.as_ptr(), w, h, w * 3, quality, &mut ptr);
        assert!(len > 0 && !ptr.is_null());
        let out = std::slice::from_raw_parts(ptr, len).to_vec();
        libwebp_sys::WebPFree(ptr.cast());
        out
    }
}

#[test]
fn d6_lossy_webp_stays_lossy_and_lossless_formats_keep_their_pixels() {
    let d6 = fixtures().join("defects/D6");
    let (_, image, lossy) = export(&d6.join("lossy-q75.webp"), MetadataPolicy::Strip);
    assert_eq!(&first_webp_image_chunk(&lossy), b"VP8 ");
    assert!(lossy == libwebp_rgb(&image.to_rgb8(), 90.0), "not q90");

    for name in ["lossless.webp", "lossless.png", "lossless.tif"] {
        let (_, image, out) = export(&d6.join(name), MetadataPolicy::Strip);
        let back = image::load_from_memory(&out).unwrap();
        assert_eq!(back, image, "{name}");
        if name.ends_with(".webp") {
            assert_eq!(&first_webp_image_chunk(&out), b"VP8L");
        }
        if name.ends_with(".tif") {
            let exif = exif_of(&out).unwrap();
            let compression = exif.get_field(Tag::Compression, In::PRIMARY).unwrap();
            assert_eq!(compression.value.get_uint(0), Some(5), "LZW");
        }
    }
}

#[derive(Deserialize)]
struct MetadataFixture {
    cases: Vec<MetadataCase>,
}

#[derive(Deserialize)]
struct MetadataCase {
    input: String,
    preserve: bool,
    gps_consent: Option<bool>,
    #[serde(default)]
    expect_exiftool_tags: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    expect_absent: Vec<String>,
}

/// exiftool `-G1 -n` names used by the fixture.
fn tag_for(name: &str) -> Option<Tag> {
    let (_, tag) = name.split_once(':')?;
    Some(match tag {
        "Make" => Tag::Make,
        "Model" => Tag::Model,
        "Orientation" => Tag::Orientation,
        "ModifyDate" => Tag::DateTime,
        "Artist" => Tag::Artist,
        "Copyright" => Tag::Copyright,
        "HostComputer" => Tag(exif::Context::Tiff, 0x013C),
        "ImageDescription" => Tag::ImageDescription,
        "Software" => Tag::Software,
        "ApertureValue" => Tag::ApertureValue,
        "ColorSpace" => Tag::ColorSpace,
        "CreateDate" => Tag::DateTimeDigitized,
        "DateTimeOriginal" => Tag::DateTimeOriginal,
        "ExposureCompensation" => Tag::ExposureBiasValue,
        "ExposureMode" => Tag::ExposureMode,
        "ExposureProgram" => Tag::ExposureProgram,
        "ExposureTime" => Tag::ExposureTime,
        "FNumber" => Tag::FNumber,
        "Flash" => Tag::Flash,
        "FocalLength" => Tag::FocalLength,
        "FocalLengthIn35mmFormat" => Tag::FocalLengthIn35mmFilm,
        "ISO" => Tag::PhotographicSensitivity,
        "LensInfo" => Tag::LensSpecification,
        "LensMake" => Tag::LensMake,
        "LensModel" => Tag::LensModel,
        "MeteringMode" => Tag::MeteringMode,
        "OffsetTime" => Tag::OffsetTime,
        "OffsetTimeDigitized" => Tag::OffsetTimeDigitized,
        "OffsetTimeOriginal" => Tag::OffsetTimeOriginal,
        "SceneCaptureType" => Tag::SceneCaptureType,
        "ShutterSpeedValue" => Tag::ShutterSpeedValue,
        "SubSecTime" => Tag::SubSecTime,
        "SubSecTimeDigitized" => Tag::SubSecTimeDigitized,
        "SubSecTimeOriginal" => Tag::SubSecTimeOriginal,
        "WhiteBalance" => Tag::WhiteBalance,
        "ImageUniqueID" => Tag::ImageUniqueID,
        "LensSerialNumber" => Tag::LensSerialNumber,
        "OwnerName" => Tag::CameraOwnerName,
        "SerialNumber" => Tag::BodySerialNumber,
        "UserComment" => Tag::UserComment,
        "MakerNoteUnknownText" => Tag::MakerNote,
        "GPSAltitude" => Tag::GPSAltitude,
        "GPSAltitudeRef" => Tag::GPSAltitudeRef,
        "GPSDateStamp" => Tag::GPSDateStamp,
        "GPSLatitude" => Tag::GPSLatitude,
        "GPSLatitudeRef" => Tag::GPSLatitudeRef,
        "GPSLongitude" => Tag::GPSLongitude,
        "GPSLongitudeRef" => Tag::GPSLongitudeRef,
        "GPSTimeStamp" => Tag::GPSTimeStamp,
        "GPSVersionID" => Tag::GPSVersionID,
        _ => return None,
    })
}

fn numbers(value: &Value) -> Vec<f64> {
    match value {
        Value::Byte(v) => v.iter().map(|&x| f64::from(x)).collect(),
        Value::Short(v) => v.iter().map(|&x| f64::from(x)).collect(),
        Value::Long(v) => v.iter().map(|&x| f64::from(x)).collect(),
        Value::Rational(v) => v.iter().map(|r| r.to_f64()).collect(),
        Value::SRational(v) => v.iter().map(|r| r.to_f64()).collect(),
        Value::Ascii(v) => v
            .iter()
            .filter_map(|s| std::str::from_utf8(s).ok()?.trim().parse().ok())
            .collect(),
        _ => Vec::new(),
    }
}

/// The field as exiftool `-n` prints it.
fn exiftool_value(tag: Tag, value: &Value) -> serde_json::Value {
    let n = numbers(value);
    let number = |v: f64| serde_json::json!(v);
    match tag {
        Tag::ApertureValue => number(2f64.powf(n[0] / 2.0)),
        Tag::ShutterSpeedValue => serde_json::json!(format!("{}", 2f64.powf(-n[0]))),
        Tag::GPSLatitude | Tag::GPSLongitude => number(n[0] + n[1] / 60.0 + n[2] / 3600.0),
        Tag::GPSTimeStamp => serde_json::json!(format!("{:02}:{:02}:{:02}", n[0], n[1], n[2])),
        Tag::GPSVersionID | Tag::LensSpecification => serde_json::json!(
            n.iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        ),
        _ => match value {
            Value::Ascii(parts) => {
                let s = String::from_utf8_lossy(&parts[0]).trim().to_string();
                s.parse::<f64>().map_or(serde_json::json!(s), number)
            }
            _ => number(n[0]),
        },
    }
}

fn same(want: &serde_json::Value, got: &serde_json::Value) -> bool {
    match (want.as_f64(), got.as_f64()) {
        (Some(a), Some(b)) => (a - b).abs() <= 1e-6 * a.abs().max(1.0),
        _ => match (want.as_str(), got.as_str()) {
            (Some(a), Some(b)) => match (a.parse::<f64>(), b.parse::<f64>()) {
                (Ok(x), Ok(y)) => (x - y).abs() <= 1e-9 * x.abs().max(1.0),
                _ => a == b,
            },
            _ => want == got,
        },
    }
}

/// The metadata fixture's tags and absences. Values are compared as exiftool `-n` prints them;
/// the fixture's comparison skips JFIF and the encoder's own tags.
#[test]
fn the_allowlist_matches_the_metadata_fixture() {
    let dir = fixtures().join("metadata");
    let spec: MetadataFixture =
        serde_json::from_str(&std::fs::read_to_string(dir.join("expected.json")).unwrap()).unwrap();
    for case in &spec.cases {
        let policy = if case.preserve {
            MetadataPolicy::Allowlist {
                gps: case.gps_consent.unwrap_or(false),
            }
        } else {
            MetadataPolicy::Strip
        };
        let (_, _, out) = export(&dir.join(&case.input), policy);
        let label = format!("{} {policy:?}", case.input);
        if !case.preserve {
            assert!(exif_of(&out).is_none(), "{label}");
            continue;
        }
        let exif = exif_of(&out).unwrap();
        for (name, want) in &case.expect_exiftool_tags {
            let tag = tag_for(name).unwrap_or_else(|| panic!("map {name}"));
            let field = exif
                .get_field(tag, In::PRIMARY)
                .unwrap_or_else(|| panic!("{label}: {name} missing"));
            let got = exiftool_value(tag, &field.value);
            assert!(same(want, &got), "{label}: {name} {got} != {want}");
        }
        for name in &case.expect_absent {
            match tag_for(name) {
                Some(tag) => assert!(
                    exif.get_field(tag, In::PRIMARY).is_none(),
                    "{label}: {name}"
                ),
                None => {
                    // IPTC and XMP: no such block in the file at all.
                    for marker in [
                        &b"Photoshop 3.0"[..],
                        b"http://ns.adobe.com/xap",
                        b"adobe:ns:meta",
                    ] {
                        assert!(
                            !out.windows(marker.len()).any(|w| w == marker),
                            "{label}: {name}"
                        );
                    }
                }
            }
        }
        assert!(
            exif.fields().all(|f| f.ifd_num == In::PRIMARY),
            "{label}: thumbnail IFD"
        );
    }
}

fn has_comment_segment(jpeg: &[u8]) -> bool {
    let mut at = 2;
    while at + 4 <= jpeg.len() && jpeg[at] == 0xFF && jpeg[at + 1] != 0xDA {
        if jpeg[at + 1] == 0xFE {
            return true;
        }
        at += 2 + usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
    }
    false
}

/// D9: "export without masking" is this re-encode; there is no byte copy of the source. Its
/// metadata follows the same policy (1.12.1 testMetadataCopyAndOrientationNormalize).
#[test]
fn d9_exports_carry_only_what_the_policy_allows() {
    let source =
        fixtures().join("legacy/tests/testMetadataCopyAndOrientationNormalize/file2-source.jpg");
    let original = std::fs::read(&source).unwrap();

    let (_, _, plain) = export(&source, MetadataPolicy::Strip);
    assert_ne!(plain, original);
    assert!(exif_of(&plain).is_none());
    assert!(!has_comment_segment(&plain));
    for marker in [&b"adobe:ns:meta"[..], b"unsafe-payload", b"TestCamera"] {
        assert!(!plain.windows(marker.len()).any(|w| w == marker));
    }

    for gps in [false, true] {
        let (_, _, kept) = export(&source, MetadataPolicy::Allowlist { gps });
        let exif = exif_of(&kept).unwrap();
        let text = |tag| {
            exif.get_field(tag, In::PRIMARY)
                .map(|f| f.display_value().to_string())
        };
        assert_eq!(text(Tag::Make).as_deref(), Some("\"TestCamera\""));
        assert_eq!(text(Tag::LensModel).as_deref(), Some("\"TestLens\""));
        let orientation = exif.get_field(Tag::Orientation, In::PRIMARY).unwrap();
        assert_eq!(orientation.value.get_uint(0), Some(1));
        for private in [
            Tag::Artist,
            Tag::CameraOwnerName,
            Tag::BodySerialNumber,
            Tag::LensSerialNumber,
            Tag::UserComment,
        ] {
            assert!(text(private).is_none(), "{private}");
        }
        assert_eq!(text(Tag::GPSLatitudeRef).is_some(), gps);
        assert!(!has_comment_segment(&kept));
        for marker in [&b"adobe:ns:meta"[..], b"unsafe-payload"] {
            assert!(!kept.windows(marker.len()).any(|w| w == marker));
        }
    }
}

/// 1.12.1 testMetadataCopyStripsEmbeddedThumbnail: the source's IFD1 thumbnail is not kept.
#[test]
fn the_embedded_thumbnail_is_dropped() {
    let source =
        fixtures().join("legacy/tests/testMetadataCopyStripsEmbeddedThumbnail/file1-source.jpg");
    let before = exif_of(&std::fs::read(&source).unwrap()).unwrap();
    assert!(before.fields().any(|f| f.ifd_num == In::THUMBNAIL));
    let (_, _, out) = export(&source, MetadataPolicy::Allowlist { gps: false });
    let exif = exif_of(&out).unwrap();
    assert!(exif.fields().all(|f| f.ifd_num == In::PRIMARY));
    let make = exif.get_field(Tag::Make, In::PRIMARY).unwrap();
    assert_eq!(make.display_value().to_string(), "\"TestCamera\"");
}

/// 1.12.1 testMetadataFailurePublishesCleanImage: a source whose EXIF block cannot be read is
/// still written, with no metadata, when metadata is kept.
#[test]
fn an_unreadable_exif_block_leaves_a_clean_output() {
    let plain = std::fs::read(fixtures().join("defects/D6/q90-420.jpg")).unwrap();
    let mut app1 = b"Exif\0\0II\x2a\0\xff\xff\xff\x7f".to_vec();
    app1.extend([0xAB; 32]);
    let mut damaged = plain[..2].to_vec();
    damaged.extend([0xFF, 0xE1]);
    damaged.extend(((app1.len() + 2) as u16).to_be_bytes());
    damaged.extend(&app1);
    damaged.extend(&plain[2..]);
    assert!(exif_of(&damaged).is_none());
    let dir = std::env::temp_dir().join(format!("cloakframe-exif-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("damaged.jpg");
    std::fs::write(&path, &damaged).unwrap();
    let (_, image, out) = export(&path, MetadataPolicy::Allowlist { gps: true });
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(exif_of(&out).is_none());
    assert!(!out.windows(6).any(|w| w == b"Exif\0\0"));
    let header = turbojpeg::read_header(&out).unwrap();
    assert_eq!(
        (header.width as u32, header.height as u32),
        (image.width(), image.height())
    );
}
