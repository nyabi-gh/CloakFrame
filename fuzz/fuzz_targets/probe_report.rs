#![no_main]

use cloakframe_core::timeline::Seconds;
use cloakframe_engine::media::probe::{
    MAX_FRAMES, MAX_PIXELS, MAX_RATE, MAX_SIDE, MAX_START_SECONDS, parse,
};
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
    assert!(spec.start <= Seconds::new(MAX_START_SECONDS.into(), 1).unwrap());
    assert!(
        spec.duration_seconds
            .is_none_or(|d| (0.0..=86_400.0).contains(&d))
    );
    assert!(spec.frame_start(1) > spec.frame_start(0));
    assert!(spec.frame_start(spec.estimated_frames) >= spec.start);
});
