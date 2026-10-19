//! The photo processor through the runner with a scripted detector: masking and publishing, P-1
//! for capped photos, the reviewer's edits, exporting without anonymization (D9), the color policy
//! on published files (D2), the source checks before publishing, and the 1.12.1 worker tests for
//! photos (tests/fixtures/legacy).

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use cloakframe_core::coverage::{CoverageEvidence, Problem};
use cloakframe_core::detection::{Detection, ModelId, Thresholds};
use cloakframe_core::geometry::Rect;
use cloakframe_core::item::{ItemId, ItemState, PublishBasis, ReviewMode};
use cloakframe_core::review::PhotoEdits;
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::{DetectError, Detector, DetectorHandle, Found, Rgb8Image};
use cloakframe_engine::photo::budget::MemoryBudget;
use cloakframe_engine::photo::color::{EmbeddedIcc, Standard, embedded_icc};
use cloakframe_engine::photo::decode::{decode, read_source};
use cloakframe_engine::photo::metadata::MetadataPolicy;
use cloakframe_engine::photo::process::{PhotoProcessor, PhotoSettings};
use cloakframe_engine::run::{Analysis, Decision, Job, JobItem, Note, RunReport, Start, start};
use exif::{In, Tag};
use image::{Rgb, RgbImage};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

struct Dir(PathBuf);

impl Dir {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let root = std::env::temp_dir().join(format!(
            "cloakframe-photo-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("in")).unwrap();
        Self(root)
    }

    fn input(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join("in").join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn out(&self, name: &str) -> PathBuf {
        self.0.join("out").join(name)
    }

    fn outputs(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.0.join("out"))
            .map(|d| {
                d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const BACKGROUND: Rgb<u8> = Rgb([200, 150, 100]);

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    RgbImage::from_pixel(width, height, BACKGROUND)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

fn face(x: f32, y: f32, w: f32, h: f32) -> Detection {
    let bbox = Rect::new(x, y, w, h).unwrap();
    Detection::new(ModelId::Yolo5FaceN, 0.9, bbox, None, None).unwrap()
}

/// Returns the same result for every image; `touch` appends to that file while detecting.
#[derive(Default)]
struct Scripted {
    detections: Vec<Detection>,
    over_limit: u32,
    touch: Option<PathBuf>,
}

impl Detector for Scripted {
    fn detect(&mut self, _: &Rgb8Image, _: &Thresholds) -> Result<Found, DetectError> {
        if let Some(path) = &self.touch {
            let mut bytes = std::fs::read(path).unwrap();
            bytes.extend_from_slice(b"edited");
            std::fs::write(path, bytes).unwrap();
        }
        Ok(Found {
            detections: self.detections.clone(),
            over_limit: self.over_limit,
        })
    }
}

fn finds(detections: Vec<Detection>) -> Scripted {
    Scripted {
        detections,
        ..Scripted::default()
    }
}

fn processor(metadata: MetadataPolicy) -> PhotoProcessor {
    PhotoProcessor::new(PhotoSettings {
        metadata,
        ..PhotoSettings::default()
    })
    .unwrap()
}

fn fresh(id: u64, input: &Path, destination: PathBuf) -> JobItem {
    JobItem {
        id: ItemId(id),
        input: input.to_path_buf(),
        destination,
        start: Start::Fresh {
            needs_review: false,
        },
    }
}

fn reviewed(
    input: &Path,
    destination: PathBuf,
    decision: Decision,
    analysis: Analysis,
    edits: PhotoEdits,
) -> JobItem {
    JobItem {
        id: ItemId(0),
        input: input.to_path_buf(),
        destination,
        start: Start::Reviewed {
            decision,
            analysis,
            edits,
        },
    }
}

fn run(
    items: Vec<JobItem>,
    review: ReviewMode,
    processor: PhotoProcessor,
    detector: Scripted,
) -> RunReport {
    let job = Job {
        items,
        review,
        workers: 2,
    };
    let detector = DetectorHandle::spawn(Box::new(detector)).unwrap();
    start(job, Arc::new(processor), detector).unwrap().wait()
}

/// Through libjpeg-turbo for JPEG, which this build's `image` does not decode.
fn rgb_of(bytes: &[u8]) -> RgbImage {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        let d = turbojpeg::decompress(bytes, turbojpeg::PixelFormat::RGB).unwrap();
        return RgbImage::from_raw(d.width as u32, d.height as u32, d.pixels).unwrap();
    }
    image::load_from_memory(bytes).unwrap().to_rgb8()
}

fn rgb(path: &Path) -> RgbImage {
    rgb_of(&std::fs::read(path).unwrap())
}

const BLACK: Rgb<u8> = Rgb([0, 0, 0]);

#[test]
fn masks_what_was_detected_and_publishes_unreviewed_in_automatic_mode() {
    let dir = Dir::new();
    let bytes = png(64, 48);
    let input = dir.input("a.png", &bytes);
    let report = run(
        vec![fresh(0, &input, dir.out("a.png"))],
        ReviewMode::Off,
        processor(MetadataPolicy::Strip),
        finds(vec![face(10.0, 10.0, 16.0, 16.0)]),
    );
    let item = &report.items[0];
    assert_eq!(
        item.state,
        ItemState::Published(PublishBasis::AutomaticUnreviewed),
        "{:?}",
        item.failure
    );
    assert_eq!(item.analysis.detections.len(), 1);
    assert!(item.analysis.evidence.is_empty() && item.notes.is_empty());
    let out = rgb(&dir.out("a.png"));
    assert_eq!(out.dimensions(), (64, 48));
    assert_eq!(*out.get_pixel(10, 10), BLACK);
    assert_eq!(*out.get_pixel(25, 25), BLACK);
    assert_eq!(*out.get_pixel(50, 40), BACKGROUND);
    assert_eq!(std::fs::read(&input).unwrap(), bytes);
}

/// P-1 with a scripted detector (the real crowd photo is in p1_photo.rs). 1.12.1
/// testDroppedDetectionsKeepTheRunOutOfCleanCompletion published such a photo with a warning;
/// now it waits for review and only an explicit export publishes it.
#[test]
fn capped_detections_hold_the_photo_until_it_is_exported_with_warnings() {
    let dir = Dir::new();
    let input = dir.input("crowd.png", &png(32, 32));
    let report = run(
        vec![fresh(0, &input, dir.out("crowd.png"))],
        ReviewMode::Off,
        processor(MetadataPolicy::Strip),
        Scripted {
            detections: vec![face(2.0, 2.0, 8.0, 8.0)],
            over_limit: 3,
            touch: None,
        },
    );
    let item = &report.items[0];
    assert_eq!(item.state, ItemState::NeedsReview);
    assert_eq!(
        item.analysis.evidence,
        [CoverageEvidence::unresolved(Problem::CappedCandidates {
            model: ModelId::Yolo5FaceN,
            frame: None,
            kept: 1,
            found: 4,
        })]
    );
    assert!(dir.outputs().is_empty());

    let report = run(
        vec![reviewed(
            &input,
            dir.out("crowd.png"),
            Decision::ExportWithWarnings,
            item.analysis.clone(),
            PhotoEdits::default(),
        )],
        ReviewMode::Required,
        processor(MetadataPolicy::Strip),
        Scripted::default(),
    );
    assert_eq!(
        report.items[0].state,
        ItemState::Published(PublishBasis::ExportedWithWarnings)
    );
    assert_eq!(report.completed(), 0);
    assert_eq!(*rgb(&dir.out("crowd.png")).get_pixel(5, 5), BLACK);
}

#[test]
fn review_edits_leave_excluded_detections_and_mask_added_areas() {
    let dir = Dir::new();
    let input = dir.input("two.png", &png(80, 40));
    let detector = || {
        finds(vec![
            face(4.0, 4.0, 12.0, 12.0),
            face(30.0, 4.0, 12.0, 12.0),
        ])
    };
    let report = run(
        vec![fresh(0, &input, dir.out("two.png"))],
        ReviewMode::Required,
        processor(MetadataPolicy::Strip),
        detector(),
    );
    assert_eq!(report.items[0].state, ItemState::NeedsReview);
    assert!(dir.outputs().is_empty());

    let edits = PhotoEdits {
        excluded: vec![0],
        added: vec![Rect::new(60.0, 20.0, 10.0, 10.0).unwrap()],
    };
    let report = run(
        vec![reviewed(
            &input,
            dir.out("two.png"),
            Decision::Approve,
            report.items[0].analysis.clone(),
            edits,
        )],
        ReviewMode::Required,
        processor(MetadataPolicy::Strip),
        Scripted::default(),
    );
    assert_eq!(
        report.items[0].state,
        ItemState::Published(PublishBasis::Reviewed)
    );
    let out = rgb(&dir.out("two.png"));
    assert_eq!(*out.get_pixel(10, 10), BACKGROUND, "excluded");
    assert_eq!(*out.get_pixel(36, 10), BLACK, "kept");
    assert_eq!(*out.get_pixel(65, 25), BLACK, "added");
}

fn exif_of(bytes: &[u8]) -> Option<exif::Exif> {
    exif::Reader::new()
        .read_from_container(&mut Cursor::new(bytes))
        .ok()
}

/// JPEG marker bytes up to the start of scan.
fn jpeg_markers(bytes: &[u8]) -> Vec<u8> {
    let mut markers = Vec::new();
    let mut at = 2;
    while at + 4 <= bytes.len() && bytes[at] == 0xFF {
        let marker = bytes[at + 1];
        markers.push(marker);
        if marker == 0xDA {
            break;
        }
        at += 2 + usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
    }
    markers
}

/// D9: "export without anonymization" re-encodes with nothing masked and under the same
/// metadata rules as any output; the source's bytes are never copied.
#[test]
fn exporting_without_anonymization_masks_nothing_and_keeps_only_allowed_metadata() {
    let source_path =
        fixtures().join("legacy/tests/testMetadataCopyAndOrientationNormalize/file2-source.jpg");
    let source_bytes = std::fs::read(&source_path).unwrap();
    let source = read_source(&source_path).unwrap();
    let expected = decode(&source, &MemoryBudget::new(1 << 30), &CancelToken::new())
        .unwrap()
        .image
        .to_rgb8();
    let mean = expected.as_raw().iter().map(|&v| u32::from(v)).sum::<u32>()
        / expected.as_raw().len() as u32;
    assert!(mean > 40, "the source is not dark");

    for policy in [
        MetadataPolicy::Strip,
        MetadataPolicy::Allowlist { gps: false },
        MetadataPolicy::Allowlist { gps: true },
    ] {
        let dir = Dir::new();
        let input = dir.input("photo.jpg", &source_bytes);
        let whole = || finds(vec![face(0.0, 0.0, 16.0, 16.0)]);
        let report = run(
            vec![fresh(0, &input, dir.out("photo.jpg"))],
            ReviewMode::Required,
            processor(policy),
            whole(),
        );
        let report = run(
            vec![reviewed(
                &input,
                dir.out("photo.jpg"),
                Decision::ExportWithoutAnonymization,
                report.items[0].analysis.clone(),
                PhotoEdits::default(),
            )],
            ReviewMode::Required,
            processor(policy),
            Scripted::default(),
        );
        assert_eq!(
            report.items[0].state,
            ItemState::Published(PublishBasis::ExportedWithoutAnonymization),
            "{:?}",
            report.items[0].failure
        );
        assert_eq!(report.completed(), 0);

        let bytes = std::fs::read(dir.out("photo.jpg")).unwrap();
        assert_ne!(bytes, source_bytes);
        let out = rgb_of(&bytes);
        let diff = out
            .as_raw()
            .iter()
            .zip(expected.as_raw())
            .map(|(a, b)| u32::from(a.abs_diff(*b)))
            .sum::<u32>()
            / out.as_raw().len() as u32;
        assert!(diff <= 6, "{policy:?}: mean difference {diff}");

        let markers = jpeg_markers(&bytes);
        assert!(!markers.contains(&0xFE), "{policy:?}: comment");
        assert!(!markers.contains(&0xED), "{policy:?}: IPTC");
        assert!(
            !bytes.windows(23).any(|w| w == b"http://ns.adobe.com/xap"),
            "{policy:?}: XMP"
        );
        assert!(
            !bytes.windows(14).any(|w| w == b"unsafe-payload"),
            "{policy:?}"
        );
        let exif = exif_of(&bytes);
        let has = |tag: Tag| {
            exif.as_ref()
                .is_some_and(|e| e.get_field(tag, In::PRIMARY).is_some())
        };
        let kept = matches!(policy, MetadataPolicy::Allowlist { .. });
        assert_eq!(has(Tag::Make), kept, "{policy:?}");
        assert_eq!(has(Tag::LensModel), kept, "{policy:?}");
        for private in [
            Tag::Artist,
            Tag::UserComment,
            Tag::CameraOwnerName,
            Tag::BodySerialNumber,
            Tag::LensSerialNumber,
        ] {
            assert!(!has(private), "{policy:?}: {private}");
        }
        assert_eq!(
            has(Tag::GPSLatitude),
            policy == MetadataPolicy::Allowlist { gps: true },
            "{policy:?}"
        );
        assert!(
            !exif
                .as_ref()
                .is_some_and(|e| e.fields().any(|f| f.ifd_num == In::THUMBNAIL)),
            "{policy:?}: thumbnail"
        );
    }
}

#[test]
fn a_source_changed_during_processing_is_not_published() {
    let dir = Dir::new();
    let input = dir.input("a.png", &png(16, 16));
    let report = run(
        vec![fresh(0, &input, dir.out("a.png"))],
        ReviewMode::Off,
        processor(MetadataPolicy::Strip),
        Scripted {
            touch: Some(input.clone()),
            ..Scripted::default()
        },
    );
    assert_eq!(report.items[0].state, ItemState::Failed);
    assert_eq!(
        report.items[0].failure.as_deref(),
        Some("the source changed while it was processed")
    );
    assert!(dir.outputs().is_empty(), "{:?}", dir.outputs());
}

#[test]
fn a_review_of_an_older_source_is_not_rendered() {
    let dir = Dir::new();
    let input = dir.input("a.png", &png(16, 16));
    let report = run(
        vec![fresh(0, &input, dir.out("a.png"))],
        ReviewMode::Required,
        processor(MetadataPolicy::Strip),
        Scripted::default(),
    );
    std::fs::write(&input, png(17, 16)).unwrap();
    let report = run(
        vec![reviewed(
            &input,
            dir.out("a.png"),
            Decision::Approve,
            report.items[0].analysis.clone(),
            PhotoEdits::default(),
        )],
        ReviewMode::Required,
        processor(MetadataPolicy::Strip),
        Scripted::default(),
    );
    assert_eq!(report.items[0].state, ItemState::Failed);
    assert_eq!(
        report.items[0].failure.as_deref(),
        Some("the source changed since it was analyzed; run it again")
    );
    assert!(dir.outputs().is_empty());
}

fn max_channel_diff(a: &RgbImage, b: &RgbImage) -> u8 {
    assert_eq!(a.dimensions(), b.dimensions());
    a.as_raw()
        .iter()
        .zip(b.as_raw())
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap()
}

/// D2 (tests/fixtures/defects/D2) on the published files: known color spaces keep their pixels
/// and carry the bundled profile, other profiles are converted to sRGB and noted, untagged photos
/// stay untagged.
#[test]
fn published_photos_follow_the_d2_color_policy() {
    const JPEG_REENCODE: u8 = 6;
    let d2 = fixtures().join("defects/D2");
    let reference = rgb(&d2.join("expected-wide-gamma18-as-srgb.png"));
    let srgb = Some(Standard::Srgb);
    let cases = [
        ("chart-srgb.png", srgb, None),
        ("chart-srgb.jpg", srgb, None),
        ("chart-display-p3.png", Some(Standard::DisplayP3), None),
        ("chart-display-p3.jpg", Some(Standard::DisplayP3), None),
        ("chart-adobe-rgb.png", Some(Standard::AdobeRgb), None),
        ("chart-adobe-rgb.jpg", Some(Standard::AdobeRgb), None),
        ("chart-wide-gamma18.png", None, Some(2)),
        ("chart-wide-gamma18.jpg", None, Some(4)),
        ("chart-untagged.png", None, None),
    ];
    let dir = Dir::new();
    let items = cases
        .iter()
        .enumerate()
        .map(|(i, (name, ..))| {
            let input = dir.input(name, &std::fs::read(d2.join(name)).unwrap());
            fresh(i as u64, &input, dir.out(name))
        })
        .collect();
    let report = run(
        items,
        ReviewMode::Off,
        processor(MetadataPolicy::Strip),
        Scripted::default(),
    );
    for ((name, profile, converted), item) in cases.iter().zip(&report.items) {
        assert_eq!(
            item.state,
            ItemState::Published(PublishBasis::AutomaticUnreviewed),
            "{name}: {:?}",
            item.failure
        );
        let out = dir.out(name);
        let embedded = embedded_icc(&read_source(&out).unwrap());
        let want = profile.map_or(EmbeddedIcc::None, |s| {
            EmbeddedIcc::Profile(s.profile_bytes().to_vec())
        });
        assert_eq!(embedded, want, "{name}");
        let pixels = rgb(&out);
        match converted {
            Some(tolerance) => {
                assert_eq!(item.notes, [Note::ColorConverted], "{name}");
                let diff = max_channel_diff(&pixels, &reference);
                assert!(diff <= *tolerance, "{name}: {diff} from Little CMS");
            }
            None => {
                assert!(item.notes.is_empty(), "{name}: {:?}", item.notes);
                let source = rgb(&d2.join(name));
                let tolerance = if name.ends_with(".jpg") {
                    JPEG_REENCODE
                } else {
                    0
                };
                let diff = max_channel_diff(&pixels, &source);
                assert!(diff <= tolerance, "{name}: pixels moved by {diff}");
            }
        }
    }
}

/// 1.12.1 testWorkerDecodesOnlySupportedContent: the content decides how a file is read, and a
/// PNG named `.jpg` is written as the JPEG its name says.
#[test]
fn only_supported_content_is_decoded_and_the_output_follows_its_name() {
    let dir = Dir::new();
    let mut ppm = b"P6\n24 24\n255\n".to_vec();
    ppm.extend([90u8, 60, 30].repeat(24 * 24));
    let netpbm = dir.input("netpbm.jpg", &ppm);
    let renamed = dir.input("renamed.jpg", &png(24, 24));
    let report = run(
        vec![
            fresh(0, &netpbm, dir.out("netpbm.jpg")),
            fresh(1, &renamed, dir.out("renamed.jpg")),
        ],
        ReviewMode::Off,
        processor(MetadataPolicy::Strip),
        Scripted::default(),
    );
    assert_eq!(report.items[0].state, ItemState::Failed);
    assert!(
        report.items[0]
            .failure
            .as_deref()
            .unwrap()
            .contains("not a JPEG, PNG, BMP, TIFF or WebP")
    );
    assert_eq!(
        report.items[1].state,
        ItemState::Published(PublishBasis::AutomaticUnreviewed)
    );
    assert_eq!(dir.outputs(), ["renamed.jpg"]);
    let out = std::fs::read(dir.out("renamed.jpg")).unwrap();
    assert!(out.starts_with(&[0xFF, 0xD8, 0xFF]));
    assert_eq!(
        turbojpeg::read_header(&out).unwrap().subsamp,
        turbojpeg::Subsamp::None
    );
}

/// 1.12.1 testWorkerAcceptsThirtyMegabyteJpeg: a 64×64 JPEG padded to 31 MiB is processed.
#[test]
fn a_thirty_one_megabyte_jpeg_is_processed() {
    let dir = Dir::new();
    let pixels = RgbImage::from_pixel(64, 64, Rgb([120, 80, 40]));
    let mut compressor = turbojpeg::Compressor::new().unwrap();
    let jpeg = compressor
        .compress_to_vec(turbojpeg::Image {
            pixels: pixels.as_raw().as_slice(),
            width: 64,
            pitch: 64 * 3,
            height: 64,
            format: turbojpeg::PixelFormat::RGB,
        })
        .unwrap();
    let input = dir.input("large.jpg", &jpeg);
    std::fs::File::options()
        .write(true)
        .open(&input)
        .unwrap()
        .set_len(31 << 20)
        .unwrap();
    let report = run(
        vec![fresh(0, &input, dir.out("large.jpg"))],
        ReviewMode::Off,
        processor(MetadataPolicy::Strip),
        Scripted::default(),
    );
    assert_eq!(
        report.items[0].state,
        ItemState::Published(PublishBasis::AutomaticUnreviewed),
        "{:?}",
        report.items[0].failure
    );
    assert_eq!(rgb(&dir.out("large.jpg")).dimensions(), (64, 64));
}

/// 1.12.1 testWorkerRejectsMultiFrameImages.
#[test]
fn a_multi_page_tiff_is_refused() {
    let dir = Dir::new();
    let bytes = std::fs::read(
        fixtures().join("legacy/tests/testWorkerRejectsMultiFrameImages/file1-input.tiff"),
    )
    .unwrap();
    let input = dir.input("pages.tiff", &bytes);
    let report = run(
        vec![fresh(0, &input, dir.out("pages.tiff"))],
        ReviewMode::Off,
        processor(MetadataPolicy::Strip),
        Scripted::default(),
    );
    assert_eq!(report.items[0].state, ItemState::Failed);
    assert!(
        report.items[0]
            .failure
            .as_deref()
            .unwrap()
            .contains("holds several images")
    );
    assert!(dir.outputs().is_empty());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A PNG with a stub IDAT, as 1.12.1 wrote it: a header that declares the size without the pixels.
fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(data);
        out.extend(&body);
        out.extend(crc32(&body).to_be_bytes());
    };
    let mut ihdr = width.to_be_bytes().to_vec();
    ihdr.extend(height.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &[0x78]);
    chunk(b"IEND", &[]);
    out
}

/// 1.12.1 testWorkerRejectsAnImageLargerThanTheMemoryBudget: refused for the memory it needs,
/// read from the header, before any pixel is decoded.
#[test]
fn an_image_larger_than_the_memory_budget_is_refused() {
    let dir = Dir::new();
    let input = dir.input("huge.png", &png_header(4000, 4000));
    let report = run(
        vec![fresh(0, &input, dir.out("huge.png"))],
        ReviewMode::Off,
        processor(MetadataPolicy::Strip).with_budget(MemoryBudget::new(64 << 20)),
        Scripted::default(),
    );
    assert_eq!(report.items[0].state, ItemState::Failed);
    let failure = report.items[0].failure.as_deref().unwrap();
    assert!(failure.contains("more than the 64 MiB"), "{failure}");
    assert!(dir.outputs().is_empty());
}
