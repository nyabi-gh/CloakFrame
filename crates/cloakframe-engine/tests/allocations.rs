//! Memory the photo pipeline allocates, counted through the global allocator: the estimate the
//! budget uses covers one image, and a compressed color profile cannot expand outside it. And
//! video tracking, which does not grow with the video's length. Own
//! binary, because it counts every allocation of the process; the tests take turns.

#![allow(unsafe_code)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use cloakframe_core::detection::{Detection, DetectionKind, ModelId, Thresholds};
use cloakframe_core::geometry::Rect;
use cloakframe_core::item::{ItemId, ItemState, PublishBasis, ReviewMode};
use cloakframe_core::mask::{MaskStyle, Method};
use cloakframe_core::timeline::Rational;
use cloakframe_core::tracking::Observation;
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::{DetectError, Detector, DetectorHandle, Found, Rgb8Image};
use cloakframe_engine::photo::budget::{MemoryBudget, PixelLayout, estimated_bytes};
use cloakframe_engine::photo::color::{EmbeddedIcc, embedded_icc};
use cloakframe_engine::photo::decode::{MAX_ICC_BYTES, decode, read_source};
use cloakframe_engine::photo::process::{PhotoProcessor, PhotoSettings};
use cloakframe_engine::run::{Job, JobItem, Start, start};
use cloakframe_engine::video::record::{Detected, Order, Record, RecordWriter, TrackRegion};
use cloakframe_engine::video::tracking::{self, KindTracking};
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

/// A synthetic record of `minutes` at 30 frames per second: four people at a time moving across
/// the frame, each replaced by a new one every 20 seconds, missed every seventh frame, with a
/// scene cut every two minutes.
fn synthetic_record(path: &std::path::Path, minutes: u64) -> u64 {
    let frames = minutes * 60 * 30;
    let mut writer = RecordWriter::<Detected>::create(path, Order::Ascending).unwrap();
    let mut entries = Vec::new();
    for k in 0..frames {
        entries.clear();
        for slot in 0..4u64 {
            if (k + slot) % 7 == 0 {
                continue;
            }
            let age = (k + slot * 150) % 600;
            let x = 50.0 + age as f32 * 2.0;
            let y = 100.0 + slot as f32 * 200.0;
            entries.push(Detected {
                model: ModelId::Yolo5FaceN,
                observation: Observation {
                    bbox: Rect::new(x, y, 60.0, 70.0).unwrap(),
                    score: if age % 5 == 0 { 0.3 } else { 0.8 },
                    roll: Some(0.1),
                },
            });
        }
        writer.frame(k, &entries).unwrap();
    }
    let cuts: Vec<u64> = (1..minutes / 2 + 1)
        .map(|i| i * 3600)
        .filter(|&c| c < frames)
        .collect();
    writer.finish(&cuts).unwrap();
    frames
}

/// Plan 9.3: memory does not grow with the video. Tracking a record and reading its regions back
/// holds the open tracks and a summary of each track: an hour peaks where ten minutes do, but for
/// the summaries of its six times as many tracks.
#[test]
fn tracking_an_hour_peaks_where_ten_minutes_do() {
    let _turn = turn();
    let dir = TestDir::new();
    let kinds = [KindTracking::new(
        DetectionKind::Face,
        0.4,
        Rational::new(30, 1).unwrap(),
    )];
    let mut peaks = Vec::new();
    for minutes in [10, 60] {
        let detections = dir.path().join(format!("detections-{minutes}"));
        let frames = synthetic_record(&detections, minutes);
        let regions = dir.path().join(format!("regions-{minutes}"));
        let mut tracks = 0;
        let mut boxes = 0u64;
        let peak = measure(|| {
            let record = Record::<Detected>::open(&detections).unwrap();
            let tracked = tracking::track(
                &record,
                &dir.path().join("backward"),
                &regions,
                &kinds,
                &CancelToken::new(),
            )
            .unwrap();
            tracks = tracked.tracks.len();
            let regions = Record::<TrackRegion>::open(&regions).unwrap();
            let mut read = regions.ascending().unwrap();
            let mut entries = Vec::new();
            while read.next_into(&mut entries).unwrap().is_some() {
                boxes += entries.len() as u64;
            }
        });
        assert!(boxes > frames * 3, "{boxes} boxes in {frames} frames");
        let size = std::fs::metadata(&detections).unwrap().len();
        println!("{minutes} min: record {size} bytes, {tracks} tracks, peak {peak} bytes");
        peaks.push((peak, tracks));
    }
    let [(short, short_tracks), (long, long_tracks)] = peaks[..] else {
        unreachable!()
    };
    assert!(
        long_tracks >= 5 * short_tracks,
        "{short_tracks} → {long_tracks} tracks"
    );
    let per_track = 512 * (long_tracks - short_tracks);
    assert!(long <= short + per_track, "{short} → {long} bytes");
}
