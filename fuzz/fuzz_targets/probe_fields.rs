#![no_main]

//! Byte-level mutation mostly breaks the JSON, so it rarely reaches the values the parser has to
//! judge. This target draws the values themselves and builds a well-formed report around them.

use cloakframe_core::timeline::Seconds;
use cloakframe_engine::media::probe::{
    MAX_FRAMES, MAX_PIXELS, MAX_RATE, MAX_SIDE, MAX_START_SECONDS, parse,
};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;
use serde_json::{Value, json};

fn pick(u: &mut Unstructured<'_>, choices: &[&str]) -> Result<Value> {
    Ok(if u.ratio(1, 8)? {
        json!(u.arbitrary::<String>()?)
    } else {
        json!(*u.choose(choices)?)
    })
}

fn fraction(u: &mut Unstructured<'_>) -> Result<Value> {
    Ok(json!(format!(
        "{}{}{}",
        u.arbitrary::<i64>()?,
        if u.arbitrary()? { "/" } else { ":" },
        u.arbitrary::<i64>()?
    )))
}

fn maybe(
    u: &mut Unstructured<'_>,
    value: impl FnOnce(&mut Unstructured<'_>) -> Result<Value>,
) -> Result<Value> {
    if u.ratio(1, 10)? {
        Ok(Value::Null)
    } else {
        value(u)
    }
}

fn stream(u: &mut Unstructured<'_>, index: usize) -> Result<Value> {
    Ok(json!({
        "index": index,
        "codec_type": pick(u, &["video", "audio", "subtitle", "data"])?,
        "codec_name": pick(u, &["h264", "hevc", "vp8", "vp9", "av1", "png", "aac"])?,
        "width": maybe(u, |u| Ok(json!(u.arbitrary::<u32>()?)))?,
        "height": maybe(u, |u| Ok(json!(u.arbitrary::<u32>()?)))?,
        "pix_fmt": pick(u, &["yuv420p", "yuvj420p", "nv12", "yuv420p10le", "gbrp", "rgb24"])?,
        "sample_aspect_ratio": maybe(u, fraction)?,
        "r_frame_rate": maybe(u, fraction)?,
        "avg_frame_rate": maybe(u, fraction)?,
        "time_base": maybe(u, fraction)?,
        "start_pts": maybe(u, |u| Ok(json!(u.arbitrary::<i64>()?)))?,
        "duration": maybe(u, |u| Ok(json!(u.arbitrary::<f64>()?.to_string())))?,
        "nb_frames": maybe(u, |u| Ok(json!(u.arbitrary::<i64>()?.to_string())))?,
        "color_space": pick(u, &["bt709", "smpte170m", "unknown", "bt2020nc"])?,
        "color_range": pick(u, &["tv", "pc", "unknown"])?,
        "color_primaries": pick(u, &["bt709", "bt2020", "unknown"])?,
        "color_transfer": pick(u, &["bt709", "smpte2084", "arib-std-b67", "unknown"])?,
        "disposition": {"attached_pic": u.int_in_range(0..=1)?},
        "side_data_list": [{"rotation": u.arbitrary::<f64>()?}],
        "tags": {"language": pick(u, &["kor", "und", "en"])?, "rotate": pick(u, &["90", "-90", "45"])?}
    }))
}

fn report(u: &mut Unstructured<'_>) -> Result<Value> {
    let count = u.int_in_range(0..=4)?;
    let streams = (0..count)
        .map(|i| stream(u, i))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "streams": streams,
        "format": {
            "format_name": pick(u, &["mov,mp4,m4a,3gp,3g2,mj2", "matroska,webm", "avi"])?,
            "duration": json!(u.arbitrary::<f64>()?.to_string())
        }
    }))
}

fuzz_target!(|data: &[u8]| {
    let Ok(report) = report(&mut Unstructured::new(data)) else {
        return;
    };
    let Ok(spec) = parse(report.to_string().as_bytes()) else {
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
