use std::fmt;

/// Decided from the bytes only. A file's name never decides how it is decoded, so a NetPBM file
/// named `.jpg` is refused and a PNG named `.jpg` is processed as the PNG it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Bmp,
    Tiff,
    Webp,
}

impl fmt::Display for ImageFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Jpeg => "JPEG",
            Self::Png => "PNG",
            Self::Bmp => "BMP",
            Self::Tiff => "TIFF",
            Self::Webp => "WebP",
        })
    }
}

impl ImageFormat {
    /// The format a file name's extension stands for, in any letter case.
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            "bmp" => Some(Self::Bmp),
            "tif" | "tiff" => Some(Self::Tiff),
            "webp" => Some(Self::Webp),
            _ => None,
        }
    }
}

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

pub fn sniff(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(ImageFormat::Jpeg)
    } else if bytes.starts_with(PNG_SIGNATURE) {
        Some(ImageFormat::Png)
    } else if bytes.len() >= 26 && bytes.starts_with(b"BM") {
        Some(ImageFormat::Bmp)
    } else if [b"II*\0", b"MM\0*", b"II+\0", b"MM\0+"]
        .iter()
        .any(|m| bytes.starts_with(*m))
    {
        Some(ImageFormat::Tiff)
    } else if bytes.len() >= 16 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else {
        None
    }
}

/// Whether the file holds more than one image (APNG, animated WebP, multi-page TIFF), found from
/// the container without decoding any frame. Such files are refused rather than processed as
/// their first frame, which would leave the others unmasked.
pub fn has_several_frames(bytes: &[u8], format: ImageFormat) -> bool {
    match format {
        ImageFormat::Png => png_frames(bytes) > 1,
        ImageFormat::Webp => webp_animated(bytes),
        ImageFormat::Tiff => tiff_has_second_ifd(bytes),
        ImageFormat::Jpeg | ImageFormat::Bmp => false,
    }
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// `num_frames` of the acTL chunk, or 1. APNG requires acTL before the first IDAT.
fn png_frames(bytes: &[u8]) -> u32 {
    let mut at = PNG_SIGNATURE.len();
    while let (Some(len), Some(kind)) = (be32(bytes, at), bytes.get(at + 4..at + 8)) {
        match kind {
            b"acTL" => return be32(bytes, at + 8).unwrap_or(1),
            b"IDAT" | b"IEND" => return 1,
            _ => at = at.saturating_add(12).saturating_add(len as usize),
        }
    }
    1
}

fn webp_animated(bytes: &[u8]) -> bool {
    let mut at = 12;
    while let Some(kind) = bytes.get(at..at + 4) {
        let Some(len) = bytes
            .get(at + 4..at + 8)
            .map(|l| u32::from_le_bytes(l.try_into().unwrap()) as usize)
        else {
            break;
        };
        match kind {
            b"VP8X" if bytes.get(at + 8).is_some_and(|flags| flags & 0x02 != 0) => return true,
            b"ANIM" | b"ANMF" => return true,
            _ => {}
        }
        at = at
            .saturating_add(8)
            .saturating_add(len)
            .saturating_add(len & 1);
    }
    false
}

fn tiff_has_second_ifd(bytes: &[u8]) -> bool {
    let little = bytes.starts_with(b"II");
    let u16_at = |at: usize| -> Option<u64> {
        let b: [u8; 2] = bytes.get(at..at + 2)?.try_into().ok()?;
        Some(u64::from(if little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        }))
    };
    let u32_at = |at: usize| -> Option<u64> {
        let b: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
        Some(u64::from(if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }))
    };
    let u64_at = |at: usize| -> Option<u64> {
        let b: [u8; 8] = bytes.get(at..at + 8)?.try_into().ok()?;
        Some(if little {
            u64::from_le_bytes(b)
        } else {
            u64::from_be_bytes(b)
        })
    };
    let next = if u16_at(2) == Some(43) {
        (|| {
            let first = usize::try_from(u64_at(8)?).ok()?;
            let entries = usize::try_from(u64_at(first)?).ok()?;
            u64_at(
                first
                    .checked_add(8)?
                    .checked_add(entries.checked_mul(20)?)?,
            )
        })()
    } else {
        (|| {
            let first = usize::try_from(u32_at(4)?).ok()?;
            let entries = usize::try_from(u16_at(first)?).ok()?;
            u32_at(
                first
                    .checked_add(2)?
                    .checked_add(entries.checked_mul(12)?)?,
            )
        })()
    };
    next.is_some_and(|offset| offset != 0)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn tiff_pages(little: bool, big: bool, pages: usize) -> Vec<u8> {
        let mut b: Vec<u8> = Vec::new();
        let put16 = |b: &mut Vec<u8>, v: u16| {
            b.extend(if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            })
        };
        let put32 = |b: &mut Vec<u8>, v: u32| {
            b.extend(if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            })
        };
        let put64 = |b: &mut Vec<u8>, v: u64| {
            b.extend(if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            })
        };
        b.extend(if little { *b"II" } else { *b"MM" });
        if big {
            put16(&mut b, 43);
            put16(&mut b, 8);
            put16(&mut b, 0);
            put64(&mut b, 16);
            for page in 0..pages {
                put64(&mut b, 0);
                let next = if page + 1 < pages {
                    b.len() as u64 + 8
                } else {
                    0
                };
                put64(&mut b, next);
            }
        } else {
            put16(&mut b, 42);
            put32(&mut b, 8);
            for page in 0..pages {
                put16(&mut b, 0);
                let next = if page + 1 < pages {
                    b.len() as u32 + 4
                } else {
                    0
                };
                put32(&mut b, next);
            }
        }
        b
    }

    fn png_with_actl(frames: u32) -> Vec<u8> {
        let mut b = PNG_SIGNATURE.to_vec();
        b.extend(8u32.to_be_bytes());
        b.extend(b"acTL");
        b.extend(frames.to_be_bytes());
        b.extend(0u32.to_be_bytes());
        b.extend(0u32.to_be_bytes());
        b
    }

    fn webp_vp8x(flags: u8) -> Vec<u8> {
        let mut b = b"RIFF\x12\0\0\0WEBPVP8X\x0a\0\0\0".to_vec();
        b.push(flags);
        b.extend([0; 9]);
        b
    }

    #[test]
    fn decides_by_magic_bytes_only() {
        assert_eq!(sniff(b"\xFF\xD8\xFF\xE0rest"), Some(ImageFormat::Jpeg));
        assert_eq!(sniff(PNG_SIGNATURE), Some(ImageFormat::Png));
        assert_eq!(sniff(&tiff_pages(true, false, 1)), Some(ImageFormat::Tiff));
        assert_eq!(sniff(&tiff_pages(false, true, 1)), Some(ImageFormat::Tiff));
        assert_eq!(sniff(&webp_vp8x(0)), Some(ImageFormat::Webp));
        assert_eq!(sniff(b"P6\n8 8\n255\n"), None);
        assert_eq!(sniff(b"\xFF\xD8"), None);
        assert_eq!(sniff(b"RIFF\0\0\0\0WAVEfmt "), None);
        assert_eq!(sniff(b"BM"), None);
    }

    #[test]
    fn finds_several_frames_without_decoding() {
        assert!(has_several_frames(&png_with_actl(3), ImageFormat::Png));
        assert!(!has_several_frames(&png_with_actl(1), ImageFormat::Png));
        assert!(has_several_frames(&webp_vp8x(0x02), ImageFormat::Webp));
        assert!(!has_several_frames(&webp_vp8x(0x10), ImageFormat::Webp));
        for (little, big) in [(true, false), (false, false), (true, true), (false, true)] {
            assert!(has_several_frames(
                &tiff_pages(little, big, 2),
                ImageFormat::Tiff
            ));
            assert!(!has_several_frames(
                &tiff_pages(little, big, 1),
                ImageFormat::Tiff
            ));
        }
    }

    #[test]
    fn truncated_containers_are_single_frame_and_do_not_panic() {
        let tiff = tiff_pages(true, false, 2);
        for len in 0..tiff.len() {
            let _ = has_several_frames(&tiff[..len], ImageFormat::Tiff);
        }
        assert!(!has_several_frames(
            &png_with_actl(3)[..18],
            ImageFormat::Png
        ));
        assert!(!has_several_frames(
            b"RIFF\0\0\0\0WEBPVP8X",
            ImageFormat::Webp
        ));
    }
}
