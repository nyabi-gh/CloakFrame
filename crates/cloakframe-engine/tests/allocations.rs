//! Memory the photo pipeline allocates, counted through the global allocator: the estimate the
//! budget uses covers one image, and a compressed color profile cannot expand outside it. Own
//! binary, because it counts every allocation of the process; the tests take turns.

#![allow(unsafe_code)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use cloakframe_core::detection::{Detection, ModelId, Thresholds};
use cloakframe_core::geometry::Rect;
use cloakframe_core::item::{ItemId, ItemState, PublishBasis, ReviewMode};
use cloakframe_core::mask::{MaskStyle, Method};
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::{DetectError, Detector, DetectorHandle, Found, Rgb8Image};
use cloakframe_engine::photo::budget::{MemoryBudget, PixelLayout, estimated_bytes};
use cloakframe_engine::photo::color::{EmbeddedIcc, embedded_icc};
use cloakframe_engine::photo::decode::{MAX_ICC_BYTES, decode, read_source};
use cloakframe_engine::photo::process::{PhotoProcessor, PhotoSettings};
use cloakframe_engine::run::{Job, JobItem, Start, start};
use common::{TestDir, fixtures, png_with_icc_of};
use image::ImageEncoder;

struct Peak;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards to the system allocator and only counts sizes.
unsafe impl GlobalAlloc for Peak {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
        PEAK.fetch_max(live, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let live = LIVE.fetch_add(new_size, Ordering::Relaxed) + new_size;
        PEAK.fetch_max(live, Ordering::Relaxed);
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        new
    }
}

#[global_allocator]
static ALLOCATOR: Peak = Peak;

/// Held for a whole test, so no other test allocates while one measures.
fn turn() -> MutexGuard<'static, ()> {
    static TURN: Mutex<()> = Mutex::new(());
    TURN.lock().unwrap_or_else(|e| e.into_inner())
}

/// The peak during `run` above what was live when it started.
fn measure(run: impl FnOnce()) -> usize {
    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    run();
    PEAK.load(Ordering::Relaxed) - before
}

const SIZE: u32 = 1024;

/// One detection over the whole image, so the mask works on every pixel.
struct WholeImage;

impl Detector for WholeImage {
    fn detect(&mut self, image: &Rgb8Image, _: &Thresholds) -> Result<Found, DetectError> {
        let bbox = Rect::new(0.0, 0.0, image.width as f32, image.height as f32).unwrap();
        Ok(Found {
            detections: vec![Detection::new(ModelId::Yolo5FaceN, 0.9, bbox, None, None).unwrap()],
            ..Found::default()
        })
    }
}

fn png(color: image::ExtendedColorType, bytes_per_pixel: usize, icc: Option<Vec<u8>>) -> Vec<u8> {
    let pixels: Vec<u8> = (0..SIZE as usize * SIZE as usize * bytes_per_pixel)
        .map(|i| (i * 31 % 251) as u8)
        .collect();
    let mut out = Cursor::new(Vec::new());
    let mut encoder = image::codecs::png::PngEncoder::new(&mut out);
    if let Some(icc) = icc {
        encoder.set_icc_profile(icc).unwrap();
    }
    encoder.write_image(&pixels, SIZE, SIZE, color).unwrap();
    out.into_inner()
}

#[test]
fn the_estimate_covers_the_peak_of_one_image() {
    let _turn = turn();
    let converted = std::fs::read(fixtures().join("defects/D2/wide-gamma18.icc")).unwrap();
    let dir = TestDir::new();
    let root = dir.path();
    let detector = DetectorHandle::spawn(Box::new(WholeImage)).unwrap();
    for (name, color, bpp, channels) in [
        ("rgb8", image::ExtendedColorType::Rgb8, 3, 3),
        ("rgba16", image::ExtendedColorType::Rgba16, 8, 4),
    ] {
        let input = root.join(format!("{name}.png"));
        std::fs::write(&input, png(color, bpp, Some(converted.clone()))).unwrap();
        for method in [Method::Fill, Method::Mosaic, Method::Blur] {
            let processor = PhotoProcessor::new(PhotoSettings {
                style: MaskStyle {
                    method,
                    soft_edges: true,
                    ..MaskStyle::default()
                },
                ..PhotoSettings::default()
            })
            .unwrap();
            let job = Job {
                items: vec![JobItem {
                    id: ItemId(0),
                    input: input.clone(),
                    destination: root.join(format!("out/{name}-{method:?}.png")),
                    start: Start::Fresh {
                        needs_review: false,
                    },
                }],
                review: ReviewMode::Off,
                workers: 1,
            };
            let mut report = None;
            let peak = measure(|| {
                report = Some(
                    start(job, Arc::new(processor), detector.clone())
                        .unwrap()
                        .wait(),
                );
            });
            let report = report.unwrap();
            assert_eq!(
                report.items[0].state,
                ItemState::Published(PublishBasis::AutomaticUnreviewed),
                "{:?}",
                report.items[0].failure
            );
            let pixels = u64::from(SIZE) * u64::from(SIZE);
            let encoded = std::fs::metadata(&input).unwrap().len();
            let layout = PixelLayout {
                bytes: bpp as u64,
                channels,
            };
            let estimate = estimated_bytes(pixels, encoded, layout);
            assert!(
                peak as u64 <= estimate,
                "{name} {method:?}: peak {:.1} B/px over the estimate of {:.1} B/px",
                peak as f64 / pixels as f64,
                estimate as f64 / pixels as f64
            );
        }
    }
}

#[test]
fn a_compressed_profile_inflates_only_to_its_limit() {
    let _turn = turn();
    let inflated = 16 * MAX_ICC_BYTES as usize;
    let dir = TestDir::new();
    let path = dir.write_bytes("bomb.png", &png_with_icc_of(inflated));
    let peak = measure(|| {
        let source = read_source(&path).unwrap();
        assert!(source.bytes.len() < 1 << 20, "{} bytes", source.bytes.len());
        decode(&source, &MemoryBudget::new(1 << 30), &CancelToken::new()).unwrap();
        assert_eq!(embedded_icc(&source), EmbeddedIcc::Unreadable);
    });
    assert!(
        peak < 3 * MAX_ICC_BYTES as usize,
        "{} MiB at the peak for a profile of {} MiB",
        peak >> 20,
        inflated >> 20
    );
}
