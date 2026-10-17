//! M0 spike: read EXIF with kamadak-exif, keep the 1.12.1 allowlist, write a fresh EXIF block
//! and put it into JPEG (APP1), PNG (eXIf), WebP (EXIF + VP8X) and TIFF (IFD0 + Exif/GPS IFDs).
//!
//! Usage: exif-writer-spike <out dir> <metadata source> [<extra image to tag>...]
//!
//! Each source becomes `<stem>.{jpg,png,webp,tif}` and `<stem>-gps.*`, encoded by the `image`
//! crate from the source pixels. Extra images (for example a lossy WebP from cwebp) are tagged
//! in place of re-encoding, so container paths the `image` crate cannot produce are covered.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use exif::{Field, In, Tag, Value};
use image::GenericImageView;

const ENTRY_LIMIT: usize = 128;
const TOTAL_LIMIT: usize = 64 * 1024;

const IFD0_ALLOWED: [u16; 4] = [0x010F, 0x0110, 0x0112, 0x0132];
// Make, Model, Orientation, DateTime above; below the Exif IFD part of the 1.12.1 list.
// Exiv2 lists ISOSpeedRatings and PhotographicSensitivity separately, but both are 0x8827.
const EXIF_ALLOWED: [u16; 26] = [
    0x9003, 0x9004, 0x9010, 0x9011, 0x9012, 0x9290, 0x9291, 0x9292, 0x829A, 0x829D, 0x8822, 0x8827, 0x9204,
    0x9207, 0x9209, 0x920A, 0xA405, 0xA402, 0xA403, 0xA406, 0xA001, 0xA433, 0xA434, 0xA432, 0x9202, 0x9201,
];

/// What the writer knows about the output it is tagging.
#[derive(Clone, Copy)]
struct Output {
    width: u32,
    height: u32,
    jpeg: bool,
    /// TIFF carries its size in IFD0; PixelX/YDimension are not allowed there.
    tiff: bool,
    /// EXIF ColorSpace: 1 for sRGB output, 0xFFFF for anything else (Display P3, Adobe RGB).
    color_space: u16,
}

#[derive(Clone, Debug)]
struct Entry {
    tag: u16,
    kind: u16,
    count: u32,
    data: Vec<u8>,
}

#[derive(Default, Debug)]
struct Kept {
    ifd0: Vec<Entry>,
    exif: Vec<Entry>,
    gps: Vec<Entry>,
}

fn encode_value(value: &Value) -> Option<(u16, u32, Vec<u8>)> {
    let mut out = Vec::new();
    let (kind, count) = match value {
        Value::Byte(v) => {
            out.extend_from_slice(v);
            (1, v.len())
        }
        Value::Ascii(parts) => {
            for part in parts {
                out.extend_from_slice(part);
                out.push(0);
            }
            (2, out.len())
        }
        Value::Short(v) => {
            v.iter().for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
            (3, v.len())
        }
        Value::Long(v) => {
            v.iter().for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
            (4, v.len())
        }
        Value::Rational(v) => {
            v.iter().for_each(|r| {
                out.extend_from_slice(&r.num.to_le_bytes());
                out.extend_from_slice(&r.denom.to_le_bytes());
            });
            (5, v.len())
        }
        Value::SByte(v) => {
            v.iter().for_each(|x| out.push(*x as u8));
            (6, v.len())
        }
        Value::SShort(v) => {
            v.iter().for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
            (8, v.len())
        }
        Value::SLong(v) => {
            v.iter().for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
            (9, v.len())
        }
        Value::SRational(v) => {
            v.iter().for_each(|r| {
                out.extend_from_slice(&r.num.to_le_bytes());
                out.extend_from_slice(&r.denom.to_le_bytes());
            });
            (10, v.len())
        }
        Value::Float(v) => {
            v.iter().for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
            (11, v.len())
        }
        Value::Double(v) => {
            v.iter().for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
            (12, v.len())
        }
        // UNDEFINED and unknown types can carry anything (MakerNote, UserComment); 1.12.1 drops them.
        Value::Undefined(..) | Value::Unknown(..) => return None,
    };
    Some((kind, count as u32, out))
}

fn keep(fields: &[Field], gps_consent: bool) -> Kept {
    let mut kept = Kept::default();
    let mut total = 0usize;
    for field in fields.iter().filter(|f| f.ifd_num == In::PRIMARY) {
        let tag = field.tag;
        let number = tag.number();
        let target = match tag.context() {
            exif::Context::Tiff if IFD0_ALLOWED.contains(&number) => &mut kept.ifd0,
            exif::Context::Exif if EXIF_ALLOWED.contains(&number) => &mut kept.exif,
            exif::Context::Gps if gps_consent => &mut kept.gps,
            _ => continue,
        };
        let Some((kind, count, data)) = encode_value(&field.value) else { continue };
        if data.len() > ENTRY_LIMIT || total + data.len() > TOTAL_LIMIT {
            continue;
        }
        total += data.len();
        target.push(Entry { tag: number, kind, count, data });
    }
    if !(kept.ifd0.is_empty() && kept.exif.is_empty() && kept.gps.is_empty()) {
        // Pixels are written upright, so whatever the camera said, the output is orientation 1.
        kept.ifd0.retain(|e| e.tag != Tag::Orientation.number());
        kept.ifd0.push(Entry { tag: 0x0112, kind: 3, count: 1, data: 1u16.to_le_bytes().to_vec() });
    }
    kept
}

/// Writes IFDs into `out`, little endian, with offsets relative to `base` (the TIFF header).
struct IfdWriter<'a> {
    out: &'a mut Vec<u8>,
    base: usize,
}

impl IfdWriter<'_> {
    fn align(&mut self) {
        while (self.out.len() - self.base) % 2 != 0 {
            self.out.push(0);
        }
    }

    fn offset(&self) -> u32 {
        (self.out.len() - self.base) as u32
    }

    /// Writes one IFD and its out-of-line values; returns its offset and, for each entry, the
    /// position of its 4-byte value field so pointer entries can be patched later.
    fn write(&mut self, entries: &[Entry], next: u32) -> (u32, Vec<(u16, usize)>) {
        self.align();
        let start = self.offset();
        let mut sorted = entries.to_vec();
        sorted.sort_by_key(|e| e.tag);
        let table_len = 2 + sorted.len() * 12 + 4;
        let mut values_at = start as usize + table_len;
        let mut values = Vec::new();
        let mut slots = Vec::new();
        self.out.extend_from_slice(&(sorted.len() as u16).to_le_bytes());
        for e in &sorted {
            self.out.extend_from_slice(&e.tag.to_le_bytes());
            self.out.extend_from_slice(&e.kind.to_le_bytes());
            self.out.extend_from_slice(&e.count.to_le_bytes());
            slots.push((e.tag, self.out.len()));
            if e.data.len() <= 4 {
                let mut inline = e.data.clone();
                inline.resize(4, 0);
                self.out.extend_from_slice(&inline);
            } else {
                self.out.extend_from_slice(&(values_at as u32).to_le_bytes());
                values.extend_from_slice(&e.data);
                values_at += e.data.len();
                if values_at % 2 != 0 {
                    values.push(0);
                    values_at += 1;
                }
            }
        }
        self.out.extend_from_slice(&next.to_le_bytes());
        self.out.extend_from_slice(&values);
        (start, slots)
    }

    fn patch(&mut self, slots: &[(u16, usize)], tag: u16, value: u32) {
        let (_, at) = slots.iter().find(|(t, _)| *t == tag).expect("pointer entry");
        self.out[*at..*at + 4].copy_from_slice(&value.to_le_bytes());
    }
}

fn pointer(tag: u16) -> Entry {
    Entry { tag, kind: 4, count: 1, data: vec![0; 4] }
}

/// Standalone EXIF block: TIFF header, IFD0, Exif IFD, GPS IFD.
fn tiff_block(kept: &Kept, output: Output) -> Vec<u8> {
    let mut ifd0 = kept.ifd0.clone();
    if output.jpeg {
        // IFD0 tags Exif 2.32 requires for a JPEG primary image; they describe the file, not the
        // photo or its owner.
        ifd0.push(rational(0x011A, 72, 1));
        ifd0.push(rational(0x011B, 72, 1));
        ifd0.push(short(0x0128, 2));
        ifd0.push(short(0x0213, 1));
    }
    let exif = exif_ifd(kept, output);
    ifd0.push(pointer(0x8769));
    if !kept.gps.is_empty() {
        ifd0.push(pointer(0x8825));
    }
    let mut out = b"II*\0\x08\0\0\0".to_vec();
    let mut w = IfdWriter { out: &mut out, base: 0 };
    let (_, slots0) = w.write(&ifd0, 0);
    let (exif_at, _) = w.write(&exif, 0);
    w.patch(&slots0, 0x8769, exif_at);
    if !kept.gps.is_empty() {
        let (gps_at, _) = w.write(&kept.gps, 0);
        w.patch(&slots0, 0x8825, gps_at);
    }
    out
}

fn short(tag: u16, value: u16) -> Entry {
    Entry { tag, kind: 3, count: 1, data: value.to_le_bytes().to_vec() }
}

fn long(tag: u16, value: u32) -> Entry {
    Entry { tag, kind: 4, count: 1, data: value.to_le_bytes().to_vec() }
}

fn rational(tag: u16, num: u32, denom: u32) -> Entry {
    Entry { tag, kind: 5, count: 1, data: [num.to_le_bytes(), denom.to_le_bytes()].concat() }
}

fn undefined(tag: u16, bytes: &[u8]) -> Entry {
    Entry { tag, kind: 7, count: bytes.len() as u32, data: bytes.to_vec() }
}

/// Copied Exif tags plus the structural ones the writer derives from the output itself.
fn exif_ifd(kept: &Kept, output: Output) -> Vec<Entry> {
    let mut exif: Vec<Entry> = kept.exif.iter().filter(|e| e.tag != 0xA001).cloned().collect();
    exif.push(undefined(0x9000, b"0232"));
    exif.push(undefined(0xA000, b"0100"));
    exif.push(short(0xA001, output.color_space));
    if !output.tiff {
        exif.push(long(0xA002, output.width));
        exif.push(long(0xA003, output.height));
    }
    if output.jpeg {
        exif.push(undefined(0x9101, &[1, 2, 3, 0]));
    }
    exif
}

fn jpeg_with_exif(jpeg: &[u8], block: &[u8]) -> Result<Vec<u8>> {
    ensure!(jpeg.starts_with(&[0xFF, 0xD8]), "not a JPEG");
    let payload_len = 2 + 6 + block.len();
    ensure!(payload_len <= 0xFFFF, "EXIF block too large for one APP1 segment");
    let mut at = 2;
    // Keep a leading JFIF APP0 first, as most writers do, then the Exif APP1.
    if jpeg.len() > 4 && jpeg[2] == 0xFF && jpeg[3] == 0xE0 {
        at += 2 + u16::from_be_bytes([jpeg[4], jpeg[5]]) as usize;
    }
    let mut out = jpeg[..at].to_vec();
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&(payload_len as u16).to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(block);
    out.extend_from_slice(&jpeg[at..]);
    Ok(out)
}

fn png_with_exif(png: &[u8], block: &[u8]) -> Result<Vec<u8>> {
    ensure!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "not a PNG");
    // eXIf must come before the first IDAT; right after IHDR is the simplest valid place.
    let ihdr_end = 8 + 4 + 4 + 13 + 4;
    let mut chunk = (block.len() as u32).to_be_bytes().to_vec();
    chunk.extend_from_slice(b"eXIf");
    chunk.extend_from_slice(block);
    let crc = crc32fast::hash(&chunk[4..]);
    chunk.extend_from_slice(&crc.to_be_bytes());
    let mut out = png[..ihdr_end].to_vec();
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&png[ihdr_end..]);
    Ok(out)
}

fn webp_with_exif(webp: &[u8], block: &[u8]) -> Result<Vec<u8>> {
    ensure!(webp.len() > 20 && &webp[..4] == b"RIFF" && &webp[8..12] == b"WEBP", "not a WebP");
    let mut chunks = Vec::new();
    let mut at = 12;
    while at + 8 <= webp.len() {
        let id: [u8; 4] = webp[at..at + 4].try_into()?;
        let len = u32::from_le_bytes(webp[at + 4..at + 8].try_into()?) as usize;
        let end = at + 8 + len;
        ensure!(end <= webp.len(), "truncated chunk");
        chunks.push((id, webp[at + 8..end].to_vec()));
        at = end + (len & 1);
    }
    let (mut flags, width, height) = match &chunks[0] {
        (id, data) if id == b"VP8X" => (data[0], u24(&data[4..7]) + 1, u24(&data[7..10]) + 1),
        (id, data) if id == b"VP8L" => {
            ensure!(data[0] == 0x2F, "bad VP8L signature");
            let bits = u32::from_le_bytes(data[1..5].try_into()?);
            let alpha = (bits >> 28) & 1 == 1;
            ((if alpha { 0x10 } else { 0 }), (bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1)
        }
        (id, data) if id == b"VP8 " => {
            ensure!(data[3..6] == [0x9D, 0x01, 0x2A], "bad VP8 start code");
            let w = u16::from_le_bytes([data[6], data[7]]) as u32 & 0x3FFF;
            let h = u16::from_le_bytes([data[8], data[9]]) as u32 & 0x3FFF;
            (0, w, h)
        }
        (id, _) => bail!("unexpected first chunk {:?}", String::from_utf8_lossy(id)),
    };
    // Any EXIF or XMP the encoder left is replaced; this writer is the only metadata source.
    chunks.retain(|(id, _)| id != b"VP8X" && id != b"EXIF" && id != b"XMP ");
    flags &= !0x04;
    flags |= 0x08;
    let mut vp8x = vec![flags, 0, 0, 0];
    vp8x.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
    vp8x.extend_from_slice(&(height - 1).to_le_bytes()[..3]);
    chunks.insert(0, (*b"VP8X", vp8x));
    chunks.push((*b"EXIF", block.to_vec()));

    let mut body = b"WEBP".to_vec();
    for (id, data) in &chunks {
        body.extend_from_slice(id);
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
        if data.len() % 2 == 1 {
            body.push(0);
        }
    }
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

fn u24(b: &[u8]) -> u32 {
    b[0] as u32 | (b[1] as u32) << 8 | (b[2] as u32) << 16
}

/// Adds the kept tags to a single-image TIFF: a new IFD0 (the encoder's entries plus Make,
/// Model, Orientation, DateTime and the Exif/GPS pointers) and the Exif and GPS IFDs are
/// appended, and the header is pointed at the new IFD0. Strip data and the encoder's
/// out-of-line values stay where they are, so their absolute offsets remain valid.
fn tiff_with_exif(tiff: &[u8], kept: &Kept, output: Output) -> Result<Vec<u8>> {
    ensure!(tiff.len() > 8 && &tiff[..4] == b"II*\0", "only little-endian classic TIFF is handled");
    let first = u32::from_le_bytes(tiff[4..8].try_into()?) as usize;
    let count = u16::from_le_bytes(tiff[first..first + 2].try_into()?) as usize;
    let mut entries = Vec::new();
    for i in 0..count {
        let e = &tiff[first + 2 + i * 12..first + 14 + i * 12];
        let tag = u16::from_le_bytes([e[0], e[1]]);
        if IFD0_ALLOWED.contains(&tag) || matches!(tag, 0x8769 | 0x8825 | 0x02BC | 0x83BB | 0x8773 | 0x927C) {
            continue;
        }
        let kind = u16::from_le_bytes([e[2], e[3]]);
        let n = u32::from_le_bytes(e[4..8].try_into()?);
        // Values over 4 bytes stay in place: copy the offset itself as the 4-byte field.
        entries.push(Entry { tag, kind, count: n, data: e[8..12].to_vec() });
    }
    let next = u32::from_le_bytes(tiff[first + 2 + count * 12..first + 6 + count * 12].try_into()?);
    ensure!(next == 0, "multi-page TIFF is rejected before this point");

    let mut out = tiff.to_vec();
    let mut w = IfdWriter { out: &mut out, base: 0 };
    let mut ifd0 = kept.ifd0.clone();
    ifd0.push(pointer(0x8769));
    if !kept.gps.is_empty() {
        ifd0.push(pointer(0x8825));
    }
    // Encoder entries keep their original 4-byte field verbatim (inline value or offset).
    let mut all = entries.clone();
    all.extend(ifd0.iter().cloned());
    let (ifd0_at, slots) = write_ifd_raw(&mut w, &all, &entries);
    let (exif_at, _) = w.write(&exif_ifd(kept, output), 0);
    w.patch(&slots, 0x8769, exif_at);
    if !kept.gps.is_empty() {
        let (gps_at, _) = w.write(&kept.gps, 0);
        w.patch(&slots, 0x8825, gps_at);
    }
    out[4..8].copy_from_slice(&ifd0_at.to_le_bytes());
    Ok(out)
}

/// Like IfdWriter::write, but entries listed in `raw` already hold their final 4-byte field.
fn write_ifd_raw(w: &mut IfdWriter, all: &[Entry], raw: &[Entry]) -> (u32, Vec<(u16, usize)>) {
    w.align();
    let start = w.offset();
    let mut sorted = all.to_vec();
    sorted.sort_by_key(|e| e.tag);
    let table_len = 2 + sorted.len() * 12 + 4;
    let mut values_at = start as usize + table_len;
    let mut values = Vec::new();
    let mut slots = Vec::new();
    w.out.extend_from_slice(&(sorted.len() as u16).to_le_bytes());
    for e in &sorted {
        w.out.extend_from_slice(&e.tag.to_le_bytes());
        w.out.extend_from_slice(&e.kind.to_le_bytes());
        w.out.extend_from_slice(&e.count.to_le_bytes());
        slots.push((e.tag, w.out.len()));
        if raw.iter().any(|r| r.tag == e.tag) || e.data.len() <= 4 {
            let mut inline = e.data.clone();
            inline.resize(4, 0);
            w.out.extend_from_slice(&inline);
        } else {
            w.out.extend_from_slice(&(values_at as u32).to_le_bytes());
            values.extend_from_slice(&e.data);
            values_at += e.data.len();
            if values_at % 2 != 0 {
                values.push(0);
                values_at += 1;
            }
        }
    }
    w.out.extend_from_slice(&0u32.to_le_bytes());
    w.out.extend_from_slice(&values);
    (start, slots)
}

fn encode(img: &image::DynamicImage, format: image::ImageFormat) -> Result<Vec<u8>> {
    let mut buf = Cursor::new(Vec::new());
    match format {
        image::ImageFormat::Jpeg => {
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 92).encode_image(img)?;
        }
        _ => img.write_to(&mut buf, format)?,
    }
    Ok(buf.into_inner())
}

fn tag_file(path: &Path, kept: &Kept) -> Result<Vec<u8>> {
    let bytes = std::fs::read(path)?;
    let (width, height) = image::load_from_memory(&bytes)?.dimensions();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    let output = Output {
        width,
        height,
        jpeg: matches!(ext.as_str(), "jpg" | "jpeg"),
        tiff: matches!(ext.as_str(), "tif" | "tiff"),
        color_space: 1,
    };
    match ext.as_str() {
        "jpg" | "jpeg" => jpeg_with_exif(&bytes, &tiff_block(kept, output)),
        "png" => png_with_exif(&bytes, &tiff_block(kept, output)),
        "webp" => webp_with_exif(&bytes, &tiff_block(kept, output)),
        "tif" | "tiff" => tiff_with_exif(&bytes, kept, output),
        _ => bail!("unsupported {ext}"),
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    ensure!(args.len() >= 3, "usage: exif-writer-spike <out dir> <metadata source> [<extra image>...]");
    let out = PathBuf::from(&args[1]);
    std::fs::create_dir_all(&out)?;
    let source = PathBuf::from(&args[2]);
    let bytes = std::fs::read(&source)?;
    let exif = exif::Reader::new().read_from_container(&mut Cursor::new(&bytes)).context("reading EXIF")?;
    let fields: Vec<Field> = exif.fields().cloned().collect();
    let stem = source.file_stem().unwrap().to_string_lossy().into_owned();
    let pixels = image::load_from_memory(&bytes)?;

    for (suffix, gps) in [("", false), ("-gps", true)] {
        let kept = keep(&fields, gps);
        println!(
            "{stem}{suffix}: kept IFD0 {} Exif {} GPS {} (source fields {}), block {} bytes",
            kept.ifd0.len(),
            kept.exif.len(),
            kept.gps.len(),
            fields.len(),
            tiff_block(&kept, Output { width: pixels.width(), height: pixels.height(), jpeg: true, tiff: false, color_space: 1 }).len()
        );
        for (ext, format) in [
            ("jpg", image::ImageFormat::Jpeg),
            ("png", image::ImageFormat::Png),
            ("webp", image::ImageFormat::WebP),
            ("tif", image::ImageFormat::Tiff),
        ] {
            let plain = out.join(format!("{stem}{suffix}.plain.{ext}"));
            std::fs::write(&plain, encode(&pixels, format)?)?;
            let tagged = tag_file(&plain, &kept)?;
            std::fs::write(out.join(format!("{stem}{suffix}.{ext}")), tagged)?;
            std::fs::remove_file(&plain)?;
        }
        for extra in &args[3..] {
            let extra = PathBuf::from(extra);
            let name = format!(
                "{stem}{suffix}.{}.{}",
                extra.file_stem().unwrap().to_string_lossy(),
                extra.extension().unwrap().to_string_lossy()
            );
            std::fs::write(out.join(name), tag_file(&extra, &kept)?)?;
        }
    }
    Ok(())
}
