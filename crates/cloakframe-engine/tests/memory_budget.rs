//! The memory estimate covers what the photo processor really allocates for one image, for the
//! largest sample format and the most expensive mask, with a profile that is converted. Own
//! binary, because it counts every allocation of the process.

#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cloakframe_core::detection::{Detection, ModelId, Thresholds};
use cloakframe_core::geometry::Rect;
use cloakframe_core::item::{ItemId, ItemState, PublishBasis, ReviewMode};
use cloakframe_core::mask::{MaskStyle, Method};
use cloakframe_engine::detect::{DetectError, Detector, DetectorHandle, Found, Rgb8Image};
use cloakframe_engine::photo::budget::{PixelLayout, estimated_bytes};
use cloakframe_engine::photo::process::{PhotoProcessor, PhotoSettings};
use cloakframe_engine::run::{Job, JobItem, Start, start};
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

const SIZE: u32 = 1024;

/// One detection over the whole image, so the mask works on every pixel.
struct WholeImage;

impl Detector for WholeImage {
    fn detect(&mut self, image: &Rgb8Image, _: &Thresholds) -> Result<Found, DetectError> {
        let bbox = Rect::new(0.0, 0.0, image.width as f32, image.height as f32).unwrap();
        Ok(Found {
            detections: vec![Detection::new(ModelId::Yolo5FaceN, 0.9, bbox, None, None).unwrap()],
            over_limit: 0,
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
    let d2 = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/defects/D2");
    let converted = std::fs::read(d2.join("wide-gamma18.icc")).unwrap();
    let root = std::env::temp_dir().join(format!("cloakframe-memory-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
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
            let before = LIVE.load(Ordering::Relaxed);
            PEAK.store(before, Ordering::Relaxed);
            let report = start(job, Arc::new(processor), detector.clone())
                .unwrap()
                .wait();
            let peak = PEAK.load(Ordering::Relaxed) - before;
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
    std::fs::remove_dir_all(root).unwrap();
}
