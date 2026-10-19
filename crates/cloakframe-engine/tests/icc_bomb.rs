//! A PNG whose iCCP inflates far beyond any real profile is read within a bounded amount of
//! memory, outside of which the memory budget does not count. Own binary, because it counts every
//! allocation of the process.

#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};

use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::photo::budget::MemoryBudget;
use cloakframe_engine::photo::color::{EmbeddedIcc, embedded_icc};
use cloakframe_engine::photo::decode::{MAX_ICC_BYTES, decode, read_source};
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
}

#[global_allocator]
static ALLOCATOR: Peak = Peak;

#[test]
fn a_compressed_profile_inflates_only_to_its_limit() {
    let inflated = 16 * MAX_ICC_BYTES as usize;
    let mut png = Cursor::new(Vec::new());
    let mut encoder = image::codecs::png::PngEncoder::new(&mut png);
    encoder.set_icc_profile(vec![0; inflated]).unwrap();
    encoder
        .write_image(&[0; 16 * 16 * 3], 16, 16, image::ExtendedColorType::Rgb8)
        .unwrap();
    let path = std::env::temp_dir().join(format!("cloakframe-icc-bomb-{}.png", std::process::id()));
    std::fs::write(&path, png.into_inner()).unwrap();

    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let source = read_source(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(source.bytes.len() < 1 << 20, "{} bytes", source.bytes.len());
    let decoded = decode(&source, &MemoryBudget::new(1 << 30), &CancelToken::new()).unwrap();
    assert_eq!(embedded_icc(&source), EmbeddedIcc::Unreadable);
    drop(decoded);

    let peak = PEAK.load(Ordering::Relaxed) - before;
    assert!(
        peak < 3 * MAX_ICC_BYTES as usize,
        "{} MiB at the peak for a profile of {} MiB",
        peak >> 20,
        inflated >> 20
    );
}
