//! Helpers shared by the engine's tests. Integration tests include this file as `mod common;`
//! and unit tests as `crate::test_support` (src/lib.rs), so it uses only the crate's
//! dependencies. Each test binary uses part of it.
#![allow(dead_code)]

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use cloakframe_core::item::{Item, ItemId, PublishPermit, ReviewMode};
use cloakframe_core::timeline::Rotation;
use image::{ImageEncoder, RgbImage};

/// A fresh directory under the system temp folder, removed on drop.
pub struct TestDir(PathBuf);

impl TestDir {
    pub fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "cloakframe-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Creates `relative` (and its parent folders) with one byte of content.
    pub fn write(&self, relative: &str) -> PathBuf {
        self.write_bytes(relative, b"x")
    }

    /// Creates `relative` (and its parent folders) holding `bytes`.
    pub fn write_bytes(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }

    pub fn mkdir(&self, relative: &str) -> PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(&path).unwrap();
        path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

/// A test input from the environment; tests fail rather than skip without it (plan 9.1).
pub fn required_env(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{name} is not set (see CONTRIBUTING.md)"))
}

/// Through libjpeg-turbo for JPEG, which this build's `image` does not decode.
pub fn rgb_of(bytes: &[u8]) -> RgbImage {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        let d = turbojpeg::decompress(bytes, turbojpeg::PixelFormat::RGB).unwrap();
        return RgbImage::from_raw(d.width as u32, d.height as u32, d.pixels).unwrap();
    }
    image::load_from_memory(bytes).unwrap().to_rgb8()
}

pub fn rgb(path: &Path) -> RgbImage {
    rgb_of(&std::fs::read(path).unwrap())
}

/// A permit for an item that went the automatic way with nothing to resolve.
pub fn permit() -> PublishPermit {
    let mut item = Item::new(ItemId(1));
    item.finish_analysis().unwrap();
    item.route(ReviewMode::Off, &[]).unwrap();
    item.start_render().unwrap();
    item.mark_verified().unwrap();
    PublishPermit::issue(&item, &[]).unwrap()
}

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A PNG with a stub IDAT, as 1.12.1 wrote it: a header that declares the size without the pixels.
pub fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(data);
        out.extend(&body);
        out.extend(crc32(&body).to_be_bytes());
    };
    let mut ihdr = width.to_be_bytes().to_vec();
    ihdr.extend(height.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &[0x78]);
    chunk(b"IEND", &[]);
    out
}

/// A 16×16 PNG whose `iCCP` inflates to `size` zero bytes, from a file of about `size / 1000`.
pub fn png_with_icc_of(size: usize) -> Vec<u8> {
    let mut png = Cursor::new(Vec::new());
    let mut encoder = image::codecs::png::PngEncoder::new(&mut png);
    encoder.set_icc_profile(vec![0; size]).unwrap();
    encoder
        .write_image(&[0; 16 * 16 * 3], 16, 16, image::ExtendedColorType::Rgb8)
        .unwrap();
    png.into_inner()
}

/// image's 40-byte BMP header rewritten as a BITMAPV5HEADER that embeds `icc` after the pixels.
pub fn bmp_v5_with_profile(bmp: &[u8], icc: &[u8]) -> Vec<u8> {
    let le32 = |at: usize| u32::from_le_bytes(bmp[at..at + 4].try_into().unwrap());
    assert_eq!(le32(14), 40);
    let rest = &bmp[14 + 40..];
    let mut header = bmp[14..14 + 40].to_vec();
    header.resize(124, 0);
    header[0..4].copy_from_slice(&124u32.to_le_bytes());
    header[56..60].copy_from_slice(b"DEBM");
    header[112..116].copy_from_slice(&((124 + rest.len()) as u32).to_le_bytes());
    header[116..120].copy_from_slice(&(icc.len() as u32).to_le_bytes());
    let mut out = b"BM".to_vec();
    out.extend(((14 + 124 + rest.len() + icc.len()) as u32).to_le_bytes());
    out.extend(&bmp[6..10]);
    out.extend((le32(10) + 84).to_le_bytes());
    out.extend(header);
    out.extend(rest);
    out.extend(icc);
    out
}

/// A gray RGB frame with `k`'s 16 bits as black and white blocks in its top rows, the numbers
/// tools/fixture-extract/gen_inputs.py draws into the P-3 fixtures.
pub fn numbered_frame(width: u32, height: u32, k: u32) -> Vec<u8> {
    let mut rgb = vec![128; (width * height * 3) as usize];
    let block = number_block(width, height);
    for bit in 0..16 {
        let value = if k >> bit & 1 == 1 { 235 } else { 16 };
        let (x, y) = ((bit % 8) * block, (bit / 8) * block);
        for row in y..y + block {
            let at = ((row * width + x) * 3) as usize;
            rgb[at..at + (block * 3) as usize].fill(value);
        }
    }
    rgb
}

/// The number [`numbered_frame`] drew into the coded frame, read from a decoded RGB frame of
/// `width`×`height` in display orientation.
pub fn frame_number(width: u32, height: u32, rgb: &[u8], rotation: Rotation) -> u32 {
    let (coded_w, coded_h) = match rotation {
        Rotation::None => (width, height),
        Rotation::ThreeQuarters => (height, width),
        other => panic!("no numbered video is turned {other:?}"),
    };
    let gray = |x: u32, y: u32| {
        let (dx, dy) = match rotation {
            Rotation::None => (x, y),
            _ => (y, coded_w - 1 - x),
        };
        let i = (dy * width + dx) as usize * 3;
        rgb[i..i + 3].iter().map(|&c| u32::from(c)).sum::<u32>() / 3
    };
    let block = number_block(coded_w, coded_h);
    (0..16)
        .filter(|bit| gray((bit % 8) * block + block / 2, (bit / 8) * block + block / 2) > 128)
        .map(|bit| 1 << bit)
        .sum()
}

fn number_block(width: u32, height: u32) -> u32 {
    (width.min(height) / 12).max(8)
}
