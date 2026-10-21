#![no_main]

use cloakframe_engine::media::probe::{MAX_FRAMES, MAX_PIXELS, MAX_RATE, MAX_SIDE, parse};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|report: &[u8]| {
    let Ok(spec) = parse(report) else {
        return;
    };
    let (width, height) = spec.display_size();
    assert!(width > 0 && height > 0 && width.max(height) <= MAX_SIDE);
    assert!(u64::from(width) * u64::from(height) <= MAX_PIXELS);
    assert!(spec.rate.as_f64() <= MAX_RATE);
    assert!(spec.estimated_frames <= MAX_FRAMES);
    assert!(spec.start.num() >= 0);
    let last = spec.frame_start(spec.estimated_frames);
    assert!(last.as_f64() >= spec.start.as_f64());
});
