//! The mask renderer against the 1.12.1 legacy fixtures (tests/fixtures/legacy) and D3.
//!
//! Rectangle fill, soft edges and the custom image match 1.12.1's pixels. The ellipse covers
//! every pixel 1.12.1's did; its boundary and the soft ellipse ramp differ by design (core
//! `mask`). Mosaic (D3) and blur (plan 5) are new methods, checked by their requirements.

mod common;

use std::path::PathBuf;

use cloakframe_core::geometry::Rect;
use cloakframe_core::mask::{MaskStyle, Method, MosaicStrength, Overlay, Padding, Region, Shape};
use cloakframe_engine::photo::mask::apply;
use common::fixtures;
use image::DynamicImage;
use serde::Deserialize;

fn legacy() -> PathBuf {
    fixtures().join("legacy/tests")
}

#[derive(Deserialize)]
struct Recorded {
    calls: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct Call {
    detections: Vec<RecordedDetection>,
    method: Option<String>,
    padding_ratio: f32,
    shape: Option<String>,
    soft_edges: Option<bool>,
    input_png: String,
    output_png: String,
    custom_image_png: Option<String>,
}

#[derive(Deserialize)]
struct RecordedDetection {
    #[serde(rename = "box")]
    bbox: [f32; 4],
    roll: f32,
    has_pose: bool,
}

/// One recorded 1.12.1 masking call: our output for its input, and 1.12.1's output.
struct Case {
    input: DynamicImage,
    ours: DynamicImage,
    theirs: DynamicImage,
}

fn load(name: &str) -> DynamicImage {
    image::open(legacy().join(name)).unwrap()
}

fn run(test: &str) -> Vec<Case> {
    let file = legacy().join(format!("{test}.json"));
    let recorded: Recorded = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    // Every masking call must be understood: one that failed to parse would otherwise drop out
    // and leave a test checking nothing.
    let cases: Vec<Case> = recorded
        .calls
        .into_iter()
        .filter(|v| matches!(v["fn"].as_str(), Some("applyAnonymization" | "applyMosaic")))
        .map(|v| serde_json::from_value::<Call>(v).unwrap_or_else(|e| panic!("{test}: {e}")))
        .map(|call| {
            let style = MaskStyle {
                method: match call.method.as_deref().unwrap_or("mosaic") {
                    "fill" => Method::Fill,
                    "blur" => Method::Blur,
                    "custom_image" => Method::CustomImage,
                    "mosaic" => Method::Mosaic,
                    other => panic!("{test}: method {other}"),
                },
                shape: match call.shape.as_deref() {
                    Some("ellipse") => Shape::Ellipse,
                    _ => Shape::Rectangle,
                },
                padding: Padding::new(call.padding_ratio).unwrap(),
                soft_edges: call.soft_edges.unwrap_or(false),
                ..MaskStyle::default()
            };
            let regions: Vec<Region> = call
                .detections
                .iter()
                .map(|d| {
                    let [x, y, w, h] = d.bbox;
                    Region {
                        roll: d.has_pose.then_some(d.roll),
                        ..Region::new(Rect::new(x, y, w, h).unwrap())
                    }
                })
                .collect();
            let overlay = call.custom_image_png.as_deref().map(|p| {
                let rgba = load(p).to_rgba8();
                Overlay::new(rgba.width(), rgba.height(), rgba.into_raw()).unwrap()
            });
            let input = load(&call.input_png);
            let mut ours = input.clone();
            apply(&mut ours, &regions, &style, overlay.as_ref()).unwrap();
            Case {
                input,
                ours,
                theirs: load(&call.output_png),
            }
        })
        .collect();
    assert!(!cases.is_empty(), "{test} records no masking call");
    cases
}

fn rgb(image: &DynamicImage, x: u32, y: u32) -> [u8; 3] {
    image.to_rgb8().get_pixel(x, y).0
}

fn assert_same_pixels(test: &str) {
    for (i, case) in run(test).iter().enumerate() {
        assert_eq!(case.ours, case.theirs, "{test}[{i}]");
    }
}

/// Largest per-sample difference from 1.12.1, in 8-bit levels.
fn max_level_diff(case: &Case) -> u16 {
    let (a, b) = (case.ours.to_rgba16(), case.theirs.to_rgba16());
    let max = a
        .as_raw()
        .iter()
        .zip(b.as_raw())
        .map(|(x, y)| x.abs_diff(*y))
        .max();
    max.unwrap_or(0).div_ceil(257)
}

#[test]
fn fill_is_opaque_on_alpha_images() {
    assert_same_pixels("testFillIsOpaqueOnAlphaImages");
}

#[test]
fn rectangle_soft_edges_match_1_12_1() {
    for test in [
        "testSoftEdgesKeepDetectedRegionFullyCovered",
        "testSoftEdgesUsePaddingForAGradualTransition",
        "testSoftEdgesAtImageBorderStayInBounds",
    ] {
        assert_same_pixels(test);
    }
    // 1.12.1 quantized the ramp to 8 bits above 2 MP.
    let large = &run("testLargeSoftEdgeMaskUsesBoundedWorkingMemoryPath")[0];
    assert!(max_level_diff(large) <= 1);
}

/// 1.12.1 testSoftEdgeMasksAgreeAcrossThreads: soft ellipses rendered on several threads at once
/// are the same as rendered alone.
#[test]
fn soft_edge_masks_agree_across_threads() {
    const TEST: &str = "testSoftEdgeMasksAgreeAcrossThreads";
    let alone = run(TEST);
    assert!(!alone.is_empty());
    let threads: Vec<_> = (0..4).map(|_| std::thread::spawn(|| run(TEST))).collect();
    for thread in threads {
        let cases = thread.join().unwrap();
        assert_eq!(cases.len(), alone.len());
        for (i, (a, b)) in alone.iter().zip(&cases).enumerate() {
            assert_eq!(a.ours, b.ours, "{TEST}[{i}]");
        }
    }
}

/// Every pixel 1.12.1 fully masked (black fill) is fully masked here.
fn covers_what_1_12_1_covered(case: &Case) {
    let (ours, theirs) = (case.ours.to_rgb8(), case.theirs.to_rgb8());
    for (x, y, p) in theirs.enumerate_pixels() {
        if p.0 == [0, 0, 0] {
            assert_eq!(ours.get_pixel(x, y).0, [0, 0, 0], "({x}, {y})");
        }
    }
}

#[test]
fn ellipses_cover_the_detected_box_and_what_1_12_1_covered() {
    let cases = run("testEllipseMasksCoverTheDetectedBoxCorners");
    for case in &cases {
        // Box (20, 20, 24, 24): an inscribed ellipse alone would miss its corners.
        for (x, y) in [(20, 20), (43, 20), (20, 43), (43, 43)] {
            assert_eq!(rgb(&case.ours, x, y), [0, 0, 0]);
        }
        assert_eq!(rgb(&case.ours, 0, 0), [100, 100, 100]);
    }
    covers_what_1_12_1_covered(&cases[0]);

    let soft = &run("testSoftEdgesEllipseKeepsCoreCovered")[0];
    for dy in -6..=6 {
        for dx in -6..=6 {
            assert_eq!(
                rgb(&soft.ours, (32 + dx) as u32, (32 + dy) as u32),
                [0, 0, 0]
            );
        }
    }
    assert_eq!(rgb(&soft.ours, 0, 0), [100, 100, 100]);
    covers_what_1_12_1_covered(soft);
}

#[test]
fn custom_images_match_1_12_1() {
    for test in [
        "testCustomImageCoversDetectedRegion",
        "testCustomImagePreservesAspectRatio",
        "testCustomImageSupports16BitOutput",
        "testOpaqueRotatedCustomImageStillCoversDetectedRegion",
        "testSemitransparentCustomImageBlendsWithOriginal",
    ] {
        assert_same_pixels(test);
    }
    // Turned images resample differently from cv::warpAffine at their edges.
    let turned = run("testCustomImageFollowsFaceRollWithoutDarkAlphaFringes");
    for case in &turned {
        assert!(max_level_diff(case) <= 2);
    }
    let l1: u64 = turned[0]
        .ours
        .to_rgb8()
        .as_raw()
        .iter()
        .zip(turned[1].ours.to_rgb8().as_raw())
        .map(|(a, b)| u64::from(a.abs_diff(*b)))
        .sum();
    assert!(l1 > 1000, "the roll changes the result");
}

#[test]
fn a_transparent_custom_image_leaves_the_mosaic_underneath() {
    let cases = run("testTransparentCustomImageFallsBackToSafeMosaic");
    assert_eq!(cases[1].ours, cases[0].ours);
    assert_ne!(cases[1].ours, cases[1].input);
    assert_eq!(rgb(&cases[1].ours, 0, 0), rgb(&cases[1].input, 0, 0));
}

fn changed(case: &Case) -> Vec<(u32, u32)> {
    let (a, b) = (case.input.to_rgb8(), case.ours.to_rgb8());
    a.enumerate_pixels()
        .filter(|(x, y, p)| b.get_pixel(*x, *y) != *p)
        .map(|(x, y, _)| (x, y))
        .collect()
}

#[test]
fn mosaic_touches_only_the_region() {
    // Box (2, 2, 4, 4) at the default strength: cells are at least 2 px, so it still changes.
    let case = &run("testApplyMosaicTouchesOnlyDetectedRegion")[0];
    let changed = changed(case);
    assert!(!changed.is_empty());
    assert!(
        changed
            .iter()
            .all(|&(x, y)| (2..6).contains(&x) && (2..6).contains(&y))
    );
}

#[test]
fn blur_hides_a_one_pixel_thin_region() {
    // Box (8, 0, 1, 16) over a column ramping 0..240 on black: the surroundings feed the kernel,
    // so the column is pulled towards them, and nothing outside the box changes.
    let case = &run("testBlurHidesARegionOnePixelThin")[0];
    assert!(changed(case).iter().all(|&(x, _)| x == 8));
    let column = |image: &DynamicImage| -> Vec<i32> {
        (0..16).map(|y| i32::from(rgb(image, 8, y)[0])).collect()
    };
    let (before, after) = (column(&case.input), column(&case.ours));
    let spread = |v: &[i32]| v.iter().max().unwrap() - v.iter().min().unwrap();
    assert!(spread(&after) * 4 <= spread(&before), "{after:?}");
}

/// D3: strength s puts s cells across the short side of a 600 px region, each the mean of the
/// pixels it covers, and every strength gives a different result.
#[test]
fn mosaic_strength_sets_the_cell_count() {
    let path = fixtures().join("defects/D3/noise-face-600.png");
    let input = image::open(path).unwrap();
    let source = input.to_rgb8();
    let region = Region::new(Rect::new(100.0, 100.0, 600.0, 600.0).unwrap());
    let mut outputs = Vec::new();
    for s in MosaicStrength::MIN..=MosaicStrength::MAX {
        let style = MaskStyle {
            method: Method::Mosaic,
            padding: Padding::new(0.0).unwrap(),
            mosaic_strength: MosaicStrength::new(s).unwrap(),
            ..MaskStyle::default()
        };
        let mut out = input.clone();
        apply(&mut out, &[region], &style, None).unwrap();
        let out = out.to_rgb8();
        let n = usize::from(s);
        let edges: Vec<u32> = (0..=n).map(|i| 100 + (i * 600 / n) as u32).collect();
        for j in 0..n {
            for i in 0..n {
                let (x0, x1, y0, y1) = (edges[i], edges[i + 1], edges[j], edges[j + 1]);
                let mut sum = [0u64; 3];
                for y in y0..y1 {
                    for x in x0..x1 {
                        let p = source.get_pixel(x, y).0;
                        assert_eq!(out.get_pixel(x, y), out.get_pixel(x0, y0), "s {s}");
                        for c in 0..3 {
                            sum[c] += u64::from(p[c]);
                        }
                    }
                }
                let count = u64::from((x1 - x0) * (y1 - y0));
                let cell = out.get_pixel(x0, y0).0;
                for c in 0..3 {
                    let mean = sum[c] as f64 / count as f64;
                    assert!(
                        (f64::from(cell[c]) - mean).abs() <= 1.0,
                        "s {s} cell {i},{j}"
                    );
                }
            }
        }
        // Neighbouring cells differ, so there are exactly n cells across.
        let row: Vec<_> = (100..700).map(|x| *out.get_pixel(x, 400)).collect();
        assert_eq!(
            row.windows(2).filter(|w| w[0] != w[1]).count() + 1,
            n,
            "s {s}"
        );
        for (x, y, p) in out.enumerate_pixels() {
            if !(100..700).contains(&x) || !(100..700).contains(&y) {
                assert_eq!(p, source.get_pixel(x, y));
            }
        }
        outputs.push(out);
    }
    for (i, a) in outputs.iter().enumerate() {
        assert!(outputs[i + 1..].iter().all(|b| a != b));
    }
}
