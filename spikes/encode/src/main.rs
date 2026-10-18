//! M2 step 0: JPEG with a chosen chroma subsampling at q92, lossy WebP at q90 (D6), and how the
//! results compare with libjpeg-turbo `cjpeg` and libwebp `cwebp` at the same settings.
//!
//!   encode-spike sampling <file.jpg>                     source sampling factors from SOF
//!   encode-spike jpeg <in> <out.jpg> <444|422|420> [icc]  encode with jpeg-encoder at q92
//!   encode-spike webp <in> <out.webp> [alpha]             encode lossy q90 with libwebp
//!   encode-spike tjpeg <in> <out.jpg> <444|422|420>      encode with libjpeg-turbo (turbojpeg)
//!   encode-spike tdecode <in.jpg> <out.png>              decode with libjpeg-turbo
//!   encode-spike psnr <reference> <candidate>             PSNR over RGB(A) and Y

use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};
use image::{DynamicImage, GenericImageView};
use jpeg_encoder::{ColorType, Encoder, SamplingFactor};

/// (component id, horizontal, vertical) for each component of the first SOF marker.
fn jpeg_sampling(bytes: &[u8]) -> Result<Vec<(u8, u8, u8)>> {
    ensure!(bytes.starts_with(&[0xFF, 0xD8]), "not a JPEG");
    let mut i = 2;
    while i + 4 <= bytes.len() {
        ensure!(bytes[i] == 0xFF, "marker expected at {i}");
        let marker = bytes[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        let is_sof = matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_sof {
            let seg = &bytes[i + 4..i + 2 + len];
            let n = seg[5] as usize;
            return Ok((0..n)
                .map(|c| {
                    let b = &seg[6 + c * 3..9 + c * 3];
                    (b[0], b[1] >> 4, b[1] & 0x0F)
                })
                .collect());
        }
        i += 2 + len;
    }
    bail!("no SOF marker")
}

fn sampling_name(factors: &[(u8, u8, u8)]) -> String {
    match factors {
        [(_, 1, 1)] => "gray".into(),
        [(_, h, v), (_, 1, 1), (_, 1, 1)] => match (h, v) {
            (1, 1) => "444".into(),
            (2, 1) => "422".into(),
            (2, 2) => "420".into(),
            (1, 2) => "440".into(),
            (4, 1) => "411".into(),
            _ => format!("{h}x{v}"),
        },
        other => format!("{other:?}"),
    }
}

fn load(path: &Path) -> Result<DynamicImage> {
    image::open(path).with_context(|| format!("decode {}", path.display()))
}

fn encode_jpeg(input: &Path, output: &Path, sampling: &str, icc: Option<&Path>) -> Result<()> {
    let rgb = load(input)?.to_rgb8();
    let factor = match sampling {
        "444" => SamplingFactor::R_4_4_4,
        "422" => SamplingFactor::R_4_2_2,
        "420" => SamplingFactor::R_4_2_0,
        other => bail!("sampling {other}"),
    };
    let mut out = Vec::new();
    let start = Instant::now();
    let mut encoder = Encoder::new(&mut out, 92);
    encoder.set_sampling_factor(factor);
    encoder.set_optimized_huffman_tables(true);
    if let Some(icc) = icc {
        encoder.add_icc_profile(&std::fs::read(icc)?)?;
    }
    encoder.encode(
        rgb.as_raw(),
        u16::try_from(rgb.width())?,
        u16::try_from(rgb.height())?,
        ColorType::Rgb,
    )?;
    let ms = start.elapsed().as_secs_f64() * 1e3;
    std::fs::write(output, &out)?;
    println!(
        "{}",
        serde_json::json!({"encoder": "jpeg-encoder", "sampling": sampling, "bytes": out.len(),
            "ms": ms, "written_sampling": sampling_name(&jpeg_sampling(&out)?)})
    );
    Ok(())
}

#[allow(unsafe_code)]
fn encode_webp(input: &Path, output: &Path, alpha: bool) -> Result<()> {
    let image = load(input)?;
    let (w, h) = image.dimensions();
    let start = Instant::now();
    let mut ptr = std::ptr::null_mut();
    // SAFETY: the buffers are tightly packed RGB8/RGBA8 of w x h pixels with the given stride;
    // libwebp allocates the output, which is copied out and released with WebPFree.
    let bytes = unsafe {
        let len = if alpha {
            let rgba = image.to_rgba8();
            libwebp_sys::WebPEncodeRGBA(rgba.as_ptr(), w as i32, h as i32, (w * 4) as i32, 90.0, &mut ptr)
        } else {
            let rgb = image.to_rgb8();
            libwebp_sys::WebPEncodeRGB(rgb.as_ptr(), w as i32, h as i32, (w * 3) as i32, 90.0, &mut ptr)
        };
        ensure!(len > 0 && !ptr.is_null(), "libwebp failed");
        let copy = std::slice::from_raw_parts(ptr, len).to_vec();
        libwebp_sys::WebPFree(ptr.cast());
        copy
    };
    let ms = start.elapsed().as_secs_f64() * 1e3;
    std::fs::write(output, &bytes)?;
    println!(
        "{}",
        serde_json::json!({"encoder": "libwebp", "alpha": alpha, "bytes": bytes.len(), "ms": ms,
            "chunk": String::from_utf8_lossy(&bytes[12..16])})
    );
    Ok(())
}

fn encode_turbo(input: &Path, output: &Path, sampling: &str) -> Result<()> {
    let rgb = load(input)?.to_rgb8();
    let subsamp = match sampling {
        "444" => turbojpeg::Subsamp::None,
        "422" => turbojpeg::Subsamp::Sub2x1,
        "420" => turbojpeg::Subsamp::Sub2x2,
        other => bail!("sampling {other}"),
    };
    let start = Instant::now();
    let mut compressor = turbojpeg::Compressor::new()?;
    compressor.set_quality(92)?;
    compressor.set_subsamp(subsamp)?;
    compressor.set_optimize(true)?;
    let image = turbojpeg::Image {
        pixels: rgb.as_raw().as_slice(),
        width: rgb.width() as usize,
        pitch: rgb.width() as usize * 3,
        height: rgb.height() as usize,
        format: turbojpeg::PixelFormat::RGB,
    };
    let out = compressor.compress_to_vec(image)?;
    let ms = start.elapsed().as_secs_f64() * 1e3;
    std::fs::write(output, &out)?;
    println!(
        "{}",
        serde_json::json!({"encoder": "turbojpeg", "sampling": sampling, "bytes": out.len(),
            "ms": ms, "written_sampling": sampling_name(&jpeg_sampling(&out)?)})
    );
    Ok(())
}

fn decode_turbo(input: &Path, output: &Path) -> Result<()> {
    let image = turbojpeg::decompress(&std::fs::read(input)?, turbojpeg::PixelFormat::RGB)?;
    image::RgbImage::from_raw(image.width as u32, image.height as u32, image.pixels)
        .context("buffer size")?
        .save(output)?;
    Ok(())
}

fn psnr(reference: &Path, candidate: &Path) -> Result<()> {
    let a = load(reference)?;
    let b = load(candidate)?;
    ensure!(a.dimensions() == b.dimensions(), "sizes differ");
    let (a, b) = (a.to_rgba8(), b.to_rgba8());
    let (mut rgb, mut y, mut alpha) = (0f64, 0f64, 0f64);
    for (p, q) in a.pixels().zip(b.pixels()) {
        for c in 0..3 {
            rgb += (f64::from(p.0[c]) - f64::from(q.0[c])).powi(2);
        }
        let luma = |v: &image::Rgba<u8>| {
            0.299 * f64::from(v.0[0]) + 0.587 * f64::from(v.0[1]) + 0.114 * f64::from(v.0[2])
        };
        y += (luma(p) - luma(q)).powi(2);
        alpha += (f64::from(p.0[3]) - f64::from(q.0[3])).powi(2);
    }
    let n = f64::from(a.width() * a.height());
    // JSON has no infinity; identical channels report null.
    let db = |mse: f64| (mse > 0.0).then(|| 10.0 * (255.0f64.powi(2) / mse).log10());
    println!(
        "{}",
        serde_json::json!({"psnr_rgb": db(rgb / (3.0 * n)), "psnr_y": db(y / n), "psnr_alpha": db(alpha / n)})
    );
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let arg = |i: usize| -> Result<&Path> { Ok(Path::new(args.get(i).context("missing argument")?)) };
    match args.get(1).map(String::as_str) {
        Some("sampling") => {
            let f = jpeg_sampling(&std::fs::read(arg(2)?)?)?;
            println!("{}", sampling_name(&f));
        }
        Some("jpeg") => encode_jpeg(arg(2)?, arg(3)?, &args[4], args.get(5).map(Path::new))?,
        Some("webp") => encode_webp(arg(2)?, arg(3)?, args.get(4).is_some_and(|a| a == "alpha"))?,
        Some("tjpeg") => encode_turbo(arg(2)?, arg(3)?, &args[4])?,
        Some("tdecode") => decode_turbo(arg(2)?, arg(3)?)?,
        Some("psnr") => psnr(arg(2)?, arg(3)?)?,
        _ => bail!("see the module comment for usage"),
    }
    Ok(())
}
