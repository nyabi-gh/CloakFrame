//! OpenCV 5.0 `cv::resize(INTER_LINEAR)` on 8-bit RGB, byte for byte with OpenCV's generic
//! (non-KleidiCV) path, so model inputs match what 1.12.1 fed its detectors (docs/M0_ORT_SPIKE.md).

use super::Rgb8Image;

const CN: usize = 3;
const COEF_SCALE: f32 = 2048.0;

/// Resizes to `width` x `height`. Same size copies; an exact 2x reduction in both directions
/// takes OpenCV's INTER_AREA fast path, as `cv::resize` does for INTER_LINEAR.
///
/// Panics if either image has no pixels.
pub fn resize_linear(src: &Rgb8Image, width: u32, height: u32) -> Rgb8Image {
    assert!(src.width > 0 && src.height > 0 && width > 0 && height > 0);
    let (sw, sh) = (src.width as usize, src.height as usize);
    let (dw, dh) = (width as usize, height as usize);
    let data = if (sw, sh) == (dw, dh) {
        src.data.clone()
    } else if sw == 2 * dw && sh == 2 * dh {
        area_half(&src.data, sw, dw, dh)
    } else {
        linear(&src.data, sw, sh, dw, dh)
    };
    Rgb8Image {
        width,
        height,
        data,
    }
}

/// `saturate_cast<short>` of an 11-bit coefficient: rounds half to even.
fn coef(v: f32) -> i32 {
    (v * COEF_SCALE).round_ties_even() as i32
}

fn linear(src: &[u8], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<u8> {
    let scale_x = sw as f64 / dw as f64;
    let scale_y = sh as f64 / dh as f64;

    // Horizontally the coefficient is pinned at the borders.
    let mut xofs = Vec::with_capacity(dw);
    let mut alpha = Vec::with_capacity(dw);
    for dx in 0..dw {
        let f = ((dx as f64 + 0.5) * scale_x - 0.5) as f32;
        let sx = f.floor();
        let (sx, fx) = if sx < 0.0 {
            (0, 0.0)
        } else if sx as usize >= sw - 1 {
            (sw - 1, 0.0)
        } else {
            (sx as usize, f - sx)
        };
        xofs.push(sx);
        alpha.push([coef(1.0 - fx), coef(fx)]);
    }
    let hrow = |sy: usize, out: &mut Vec<i32>| {
        let row = &src[sy * sw * CN..(sy + 1) * sw * CN];
        for (&sx, a) in xofs.iter().zip(&alpha) {
            for c in 0..CN {
                let p0 = i32::from(row[sx * CN + c]);
                let p1 = if sx + 1 < sw {
                    i32::from(row[(sx + 1) * CN + c])
                } else {
                    0
                };
                out.push(p0 * a[0] + p1 * a[1]);
            }
        }
    };

    let row_len = dw * CN;
    // OpenCV finishes a row with an overlapping vector rather than the scalar formula, so every
    // row of at least 8 elements uses the vector one.
    let vector = row_len >= 8;
    let mut cache: Vec<(usize, Vec<i32>)> = Vec::with_capacity(2);
    let mut out = vec![0u8; dh * row_len];
    for (dy, dst) in out.chunks_exact_mut(row_len).enumerate() {
        // Vertically the coefficient keeps its fraction at the borders; only the row is clamped.
        let f = ((dy as f64 + 0.5) * scale_y - 0.5) as f32;
        let sy = f.floor();
        let fy = f - sy;
        let (b0, b1) = (coef(1.0 - fy), coef(fy));
        let clamp = |v: f32| (v.max(0.0) as usize).min(sh - 1);
        let rows = [clamp(sy), clamp(sy + 1.0)];
        cache.retain(|(r, _)| rows.contains(r));
        for r in rows {
            if !cache.iter().any(|(c, _)| *c == r) {
                let mut buf = Vec::with_capacity(row_len);
                hrow(r, &mut buf);
                cache.push((r, buf));
            }
        }
        let row = |r: usize| &cache.iter().find(|(c, _)| *c == r).expect("cached above").1;
        let (r0, r1) = (row(rows[0]), row(rows[1]));
        for (x, d) in dst.iter_mut().enumerate() {
            let v = if vector {
                ((((r0[x] >> 4) * b0) >> 16) + (((r1[x] >> 4) * b1) >> 16) + 2) >> 2
            } else {
                (r0[x] * b0 + r1[x] * b1 + (1 << 21)) >> 22
            };
            *d = v.clamp(0, 255) as u8;
        }
    }
    out
}

/// `resizeAreaFast` for 2x2 blocks. Both the vector loop and `ResizeAreaFastVec`'s scalar tail
/// round half up, so the generic `saturate_cast` (half to even) path is never reached.
fn area_half(src: &[u8], sw: usize, dw: usize, dh: usize) -> Vec<u8> {
    let row_len = dw * CN;
    let mut out = vec![0u8; dh * row_len];
    for (dy, dst) in out.chunks_exact_mut(row_len).enumerate() {
        let s0 = &src[2 * dy * sw * CN..(2 * dy + 1) * sw * CN];
        let s1 = &src[(2 * dy + 1) * sw * CN..(2 * dy + 2) * sw * CN];
        for (i, d) in dst.iter_mut().enumerate() {
            let (px, c) = (i / CN, i % CN);
            let at = 2 * px * CN + c;
            let sum = u32::from(s0[at]) + u32::from(s0[at + CN]) + u32::from(s1[at]);
            let sum = sum + u32::from(s1[at + CN]);
            *d = ((sum + 2) >> 2) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use sha2::{Digest, Sha256};

    /// The source pattern of tools/resize-reference/make.py.
    fn pattern(width: u32, height: u32) -> Rgb8Image {
        let mut data = Vec::with_capacity(width as usize * height as usize * CN);
        for y in 0..height {
            for x in 0..width {
                for c in 0..CN as u32 {
                    let mut v = x
                        .wrapping_mul(0x9E37_79B1)
                        .wrapping_add(y.wrapping_mul(0x85EB_CA77))
                        .wrapping_add(c.wrapping_mul(0xC2B2_AE3D));
                    v ^= v >> 15;
                    v = v.wrapping_mul(0x2C1B_3C6D);
                    v ^= v >> 12;
                    data.push((v >> 24) as u8);
                }
            }
        }
        Rgb8Image {
            width,
            height,
            data,
        }
    }

    #[derive(Deserialize)]
    struct Case {
        src: [u32; 2],
        dst: [u32; 2],
        sha256: String,
    }

    #[derive(Deserialize)]
    struct Reference {
        cases: Vec<Case>,
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn matches_opencv_on_every_reference_case() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/resize/opencv-5.0.0-x86_64.json"
        );
        let reference: Reference =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(reference.cases.len() >= 90);
        let area = reference
            .cases
            .iter()
            .filter(|c| c.src == [c.dst[0] * 2, c.dst[1] * 2])
            .count();
        assert!(area >= 8, "{area} exact 2x cases");
        let failing: Vec<String> = reference
            .cases
            .iter()
            .filter(|case| {
                let out =
                    resize_linear(&pattern(case.src[0], case.src[1]), case.dst[0], case.dst[1]);
                hex(&Sha256::digest(&out.data)) != case.sha256
            })
            .map(|c| format!("{:?} -> {:?}", c.src, c.dst))
            .collect();
        assert!(failing.is_empty(), "differs from OpenCV: {failing:?}");
    }

    #[test]
    fn rounding_rules_that_the_reference_relies_on() {
        let one_row = |data: Vec<u8>| Rgb8Image {
            width: (data.len() / CN) as u32,
            height: 1,
            data,
        };
        // 2x2 blocks summing to 10 (2.5) round up across the vector part and the tail.
        let mut src = vec![0u8; 2 * 34 * CN];
        for (i, v) in src.iter_mut().enumerate() {
            let px = (i / CN) % 34;
            *v = if px.is_multiple_of(2) { 2 } else { 3 };
        }
        let src = Rgb8Image {
            width: 34,
            height: 2,
            data: src,
        };
        let out = resize_linear(&src, 17, 1);
        assert!(out.data.iter().all(|&v| v == 3));
        // Same size is a copy.
        let row = one_row(vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(resize_linear(&row, 2, 1), row);
    }
}
