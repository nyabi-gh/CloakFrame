//! Output metadata: the 1.12.1 allowlist of EXIF tags, written as a fresh EXIF block, and the
//! bundled ICC profile, put into JPEG, WebP and TIFF bytes (PNG goes through its encoder).
//! Nothing else from the source is carried over (docs/M0_EXIF_WRITER.md).

use std::io::Cursor;

use exif::{Field, In, Value};

/// What the user chose to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MetadataPolicy {
    /// No EXIF, XMP, IPTC or comments at all. The default.
    #[default]
    Strip,
    /// The allowlisted camera and exposure tags; GPS only with its own consent.
    Allowlist { gps: bool },
}

const ENTRY_LIMIT: usize = 128;
/// One JPEG APP1 segment holds 65,533 bytes after "Exif\0\0"; the block must fit it.
const BLOCK_LIMIT: usize = 65_527;

/// Make, Model, Orientation, DateTime.
const IFD0_ALLOWED: [u16; 4] = [0x010F, 0x0110, 0x0112, 0x0132];
/// The Exif IFD part of 1.12.1's 31 Exiv2 keys (ISOSpeedRatings and PhotographicSensitivity are
/// both 0x8827).
const EXIF_ALLOWED: [u16; 26] = [
    0x9003, 0x9004, 0x9010, 0x9011, 0x9012, 0x9290, 0x9291, 0x9292, 0x829A, 0x829D, 0x8822, 0x8827,
    0x9204, 0x9207, 0x9209, 0x920A, 0xA405, 0xA402, 0xA403, 0xA406, 0xA001, 0xA433, 0xA434, 0xA432,
    0x9202, 0x9201,
];

const TAG_ORIENTATION: u16 = 0x0112;
const TAG_EXIF_IFD: u16 = 0x8769;
const TAG_GPS_IFD: u16 = 0x8825;
const TAG_COLOR_SPACE: u16 = 0xA001;
const TAG_ICC_PROFILE: u16 = 34675;

const TYPE_BYTE: u16 = 1;
const TYPE_SHORT: u16 = 3;
const TYPE_LONG: u16 = 4;
const TYPE_RATIONAL: u16 = 5;
const TYPE_UNDEFINED: u16 = 7;

/// What the writer knows about the file it is tagging.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Output {
    pub width: u32,
    pub height: u32,
    pub container: Container,
    /// EXIF ColorSpace: 1 for sRGB pixels, 0xFFFF for anything else (Display P3, Adobe RGB).
    pub srgb: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Container {
    Jpeg,
    Png,
    Webp,
    Tiff,
}

#[derive(Debug, Clone, PartialEq)]
struct Entry {
    tag: u16,
    kind: u16,
    count: u32,
    data: Vec<u8>,
}

/// The source tags that survive the allowlist.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Kept {
    ifd0: Vec<Entry>,
    exif: Vec<Entry>,
    gps: Vec<Entry>,
}

impl Kept {
    pub(crate) fn is_empty(&self) -> bool {
        self.ifd0.is_empty() && self.exif.is_empty() && self.gps.is_empty()
    }
}

fn encode_value(value: &Value) -> Option<(u16, u32, Vec<u8>)> {
    let mut out = Vec::new();
    let (kind, count) = match value {
        Value::Byte(v) => {
            out.extend_from_slice(v);
            (TYPE_BYTE, v.len())
        }
        Value::Ascii(parts) => {
            for part in parts {
                out.extend_from_slice(part);
                out.push(0);
            }
            (2, out.len())
        }
        Value::Short(v) => {
            v.iter()
                .for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
            (TYPE_SHORT, v.len())
        }
        Value::Long(v) => {
            v.iter()
                .for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
            (TYPE_LONG, v.len())
        }
        Value::Rational(v) => {
            v.iter().for_each(|r| {
                out.extend_from_slice(&r.num.to_le_bytes());
                out.extend_from_slice(&r.denom.to_le_bytes());
            });
            (TYPE_RATIONAL, v.len())
        }
        Value::SByte(v) => {
            v.iter()
                .for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
            (6, v.len())
        }
        Value::SShort(v) => {
            v.iter()
                .for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
            (8, v.len())
        }
        Value::SLong(v) => {
            v.iter()
                .for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
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
            v.iter()
                .for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
            (11, v.len())
        }
        Value::Double(v) => {
            v.iter()
                .for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
            (12, v.len())
        }
        // UNDEFINED and unknown types can carry anything (MakerNote, UserComment); 1.12.1 drops them.
        Value::Undefined(..) | Value::Unknown(..) => return None,
    };
    Some((kind, u32::try_from(count).ok()?, out))
}

/// The allowlisted tags of the source's primary image. Unreadable EXIF keeps nothing: the
/// output is then published without metadata, as 1.12.1 did.
pub(crate) fn keep(source: &[u8], policy: MetadataPolicy) -> Kept {
    let MetadataPolicy::Allowlist { gps } = policy else {
        return Kept::default();
    };
    let Ok(exif) = exif::Reader::new().read_from_container(&mut Cursor::new(source)) else {
        return Kept::default();
    };
    let fields: Vec<&Field> = exif.fields().filter(|f| f.ifd_num == In::PRIMARY).collect();
    let mut kept = Kept::default();
    let mut total = 0usize;
    for field in fields {
        let number = field.tag.number();
        let target = match field.tag.context() {
            exif::Context::Tiff if IFD0_ALLOWED.contains(&number) => &mut kept.ifd0,
            exif::Context::Exif if EXIF_ALLOWED.contains(&number) => &mut kept.exif,
            exif::Context::Gps if gps => &mut kept.gps,
            _ => continue,
        };
        let Some((kind, count, data)) = encode_value(&field.value) else {
            continue;
        };
        // 12 bytes per entry plus the value. The structural tags and IFD headers fit in the 1 KiB
        // left for them.
        let cost = 12 + data.len();
        if data.len() > ENTRY_LIMIT || total + cost > BLOCK_LIMIT - 1024 {
            continue;
        }
        total += cost;
        target.push(Entry {
            tag: number,
            kind,
            count,
            data,
        });
    }
    if !kept.is_empty() {
        // Pixels are written upright, so whatever the camera said, the output is orientation 1.
        kept.ifd0.retain(|e| e.tag != TAG_ORIENTATION);
        kept.ifd0.push(short(TAG_ORIENTATION, 1));
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
        if !(self.out.len() - self.base).is_multiple_of(2) {
            self.out.push(0);
        }
    }

    fn offset(&self) -> u32 {
        (self.out.len() - self.base) as u32
    }

    /// Writes one IFD and its out-of-line values. Entries listed in `verbatim` already hold their
    /// final 4-byte field (an inline value or an offset into the file). Returns the IFD's offset
    /// and the position of each entry's value field, for patching pointers.
    fn write(&mut self, entries: &[Entry], verbatim: &[u16]) -> (u32, Vec<(u16, usize)>) {
        self.align();
        let start = self.offset();
        let mut sorted = entries.to_vec();
        sorted.sort_by_key(|e| e.tag);
        let mut values_at = start as usize + 2 + sorted.len() * 12 + 4;
        let mut values = Vec::new();
        let mut slots = Vec::new();
        self.out
            .extend_from_slice(&(sorted.len() as u16).to_le_bytes());
        for e in &sorted {
            self.out.extend_from_slice(&e.tag.to_le_bytes());
            self.out.extend_from_slice(&e.kind.to_le_bytes());
            self.out.extend_from_slice(&e.count.to_le_bytes());
            slots.push((e.tag, self.out.len()));
            if verbatim.contains(&e.tag) || e.data.len() <= 4 {
                let mut inline = e.data.clone();
                inline.resize(4, 0);
                self.out.extend_from_slice(&inline);
            } else {
                self.out
                    .extend_from_slice(&(values_at as u32).to_le_bytes());
                values.extend_from_slice(&e.data);
                values_at += e.data.len();
                if !values_at.is_multiple_of(2) {
                    values.push(0);
                    values_at += 1;
                }
            }
        }
        self.out.extend_from_slice(&0u32.to_le_bytes());
        self.out.extend_from_slice(&values);
        (start, slots)
    }

    fn patch(&mut self, slots: &[(u16, usize)], tag: u16, value: u32) {
        let (_, at) = slots
            .iter()
            .find(|(t, _)| *t == tag)
            .expect("pointer entry written");
        self.out[*at..*at + 4].copy_from_slice(&value.to_le_bytes());
    }
}

fn short(tag: u16, value: u16) -> Entry {
    Entry {
        tag,
        kind: TYPE_SHORT,
        count: 1,
        data: value.to_le_bytes().to_vec(),
    }
}

fn long(tag: u16, value: u32) -> Entry {
    Entry {
        tag,
        kind: TYPE_LONG,
        count: 1,
        data: value.to_le_bytes().to_vec(),
    }
}

fn rational(tag: u16, num: u32, denom: u32) -> Entry {
    Entry {
        tag,
        kind: TYPE_RATIONAL,
        count: 1,
        data: [num.to_le_bytes(), denom.to_le_bytes()].concat(),
    }
}

fn undefined(tag: u16, bytes: &[u8]) -> Entry {
    Entry {
        tag,
        kind: TYPE_UNDEFINED,
        count: bytes.len() as u32,
        data: bytes.to_vec(),
    }
}

fn pointer(tag: u16) -> Entry {
    long(tag, 0)
}

/// Copied Exif tags plus the structural ones derived from the output itself, never copied:
/// ColorSpace must describe the output (a kept Display P3 file is not sRGB).
fn exif_ifd(kept: &Kept, output: Output) -> Vec<Entry> {
    let mut exif: Vec<Entry> = kept
        .exif
        .iter()
        .filter(|e| e.tag != TAG_COLOR_SPACE)
        .cloned()
        .collect();
    exif.push(undefined(0x9000, b"0232"));
    exif.push(undefined(0xA000, b"0100"));
    exif.push(short(TAG_COLOR_SPACE, if output.srgb { 1 } else { 0xFFFF }));
    if output.container != Container::Tiff {
        exif.push(long(0xA002, output.width));
        exif.push(long(0xA003, output.height));
    }
    if output.container == Container::Jpeg {
        exif.push(undefined(0x9101, &[1, 2, 3, 0]));
    }
    exif
}

/// The standalone EXIF block (TIFF header, IFD0, Exif IFD, GPS IFD) for JPEG, PNG and WebP.
pub(crate) fn exif_block(kept: &Kept, output: Output) -> Vec<u8> {
    let mut ifd0 = kept.ifd0.clone();
    if output.container == Container::Jpeg {
        // IFD0 tags Exif 2.32 requires for a JPEG primary image; they describe the file only.
        ifd0.push(rational(0x011A, 72, 1));
        ifd0.push(rational(0x011B, 72, 1));
        ifd0.push(short(0x0128, 2));
        ifd0.push(short(0x0213, 1));
    }
    ifd0.push(pointer(TAG_EXIF_IFD));
    if !kept.gps.is_empty() {
        ifd0.push(pointer(TAG_GPS_IFD));
    }
    let mut out = b"II*\0\x08\0\0\0".to_vec();
    let mut w = IfdWriter {
        out: &mut out,
        base: 0,
    };
    let (_, slots) = w.write(&ifd0, &[]);
    let (exif_at, _) = w.write(&exif_ifd(kept, output), &[]);
    w.patch(&slots, TAG_EXIF_IFD, exif_at);
    if !kept.gps.is_empty() {
        let (gps_at, _) = w.write(&kept.gps, &[]);
        w.patch(&slots, TAG_GPS_IFD, gps_at);
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerError(pub &'static str);

impl std::fmt::Display for ContainerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "could not add metadata: {}", self.0)
    }
}

impl std::error::Error for ContainerError {}

/// Inserts APP1 Exif and APP2 ICC_PROFILE after SOI (and a leading JFIF APP0).
pub(crate) fn jpeg_with(
    jpeg: &[u8],
    exif: Option<&[u8]>,
    icc: Option<&[u8]>,
) -> Result<Vec<u8>, ContainerError> {
    if !jpeg.starts_with(&[0xFF, 0xD8]) {
        return Err(ContainerError("not a JPEG"));
    }
    let mut at = 2;
    if jpeg.len() > 6 && jpeg[2] == 0xFF && jpeg[3] == 0xE0 {
        at += 2 + usize::from(u16::from_be_bytes([jpeg[4], jpeg[5]]));
    }
    let mut out = jpeg[..at].to_vec();
    let mut segment = |marker: u8, payload: &[&[u8]]| -> Result<(), ContainerError> {
        let len = 2 + payload.iter().map(|p| p.len()).sum::<usize>();
        let len = u16::try_from(len).map_err(|_| ContainerError("segment too large"))?;
        out.extend_from_slice(&[0xFF, marker]);
        out.extend_from_slice(&len.to_be_bytes());
        payload.iter().for_each(|p| out.extend_from_slice(p));
        Ok(())
    };
    if let Some(block) = exif {
        segment(0xE1, &[b"Exif\0\0", block])?;
    }
    if let Some(profile) = icc {
        // ICC.1 B.4: chunks of at most 65,519 bytes, numbered from 1.
        let chunks: Vec<&[u8]> = profile.chunks(65_519).collect();
        let total = u8::try_from(chunks.len()).map_err(|_| ContainerError("ICC too large"))?;
        for (i, chunk) in chunks.iter().enumerate() {
            segment(0xE2, &[b"ICC_PROFILE\0", &[i as u8 + 1, total], chunk])?;
        }
    }
    out.extend_from_slice(&jpeg[at..]);
    Ok(out)
}

fn u24(b: &[u8]) -> u32 {
    u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16
}

/// Rebuilds a WebP as VP8X + ICCP + image chunks + EXIF, dropping any metadata chunk libwebp
/// or the source left. A file with neither stays a simple WebP.
pub(crate) fn webp_with(
    webp: &[u8],
    exif: Option<&[u8]>,
    icc: Option<&[u8]>,
) -> Result<Vec<u8>, ContainerError> {
    if webp.len() < 20 || &webp[..4] != b"RIFF" || &webp[8..12] != b"WEBP" {
        return Err(ContainerError("not a WebP"));
    }
    let mut chunks: Vec<([u8; 4], &[u8])> = Vec::new();
    let mut at = 12;
    while at + 8 <= webp.len() {
        let id: [u8; 4] = webp[at..at + 4].try_into().expect("4 bytes");
        let len = u32::from_le_bytes(webp[at + 4..at + 8].try_into().expect("4 bytes")) as usize;
        let end = at + 8 + len;
        if end > webp.len() {
            return Err(ContainerError("truncated WebP chunk"));
        }
        chunks.push((id, &webp[at + 8..end]));
        at = end + (len & 1);
    }
    let Some(&(first, data)) = chunks.first() else {
        return Err(ContainerError("empty WebP"));
    };
    let (alpha, width, height) = match &first {
        b"VP8X" if data.len() >= 10 => (
            data[0] & 0x10 != 0,
            u24(&data[4..7]) + 1,
            u24(&data[7..10]) + 1,
        ),
        b"VP8L" if data.len() >= 5 && data[0] == 0x2F => {
            let bits = u32::from_le_bytes(data[1..5].try_into().expect("4 bytes"));
            (
                (bits >> 28) & 1 == 1,
                (bits & 0x3FFF) + 1,
                ((bits >> 14) & 0x3FFF) + 1,
            )
        }
        b"VP8 " if data.len() >= 10 && data[3..6] == [0x9D, 0x01, 0x2A] => (
            false,
            u32::from(u16::from_le_bytes([data[6], data[7]]) & 0x3FFF),
            u32::from(u16::from_le_bytes([data[8], data[9]]) & 0x3FFF),
        ),
        _ => return Err(ContainerError("unexpected first WebP chunk")),
    };
    chunks.retain(|(id, _)| !matches!(id, b"VP8X" | b"ICCP" | b"EXIF" | b"XMP "));
    if exif.is_none() && icc.is_none() && !alpha {
        let mut body = b"WEBP".to_vec();
        append_chunks(&mut body, &chunks);
        return Ok(riff(body));
    }
    let mut flags = 0u8;
    if icc.is_some() {
        flags |= 0x20;
    }
    if alpha {
        flags |= 0x10;
    }
    if exif.is_some() {
        flags |= 0x08;
    }
    let mut vp8x = vec![flags, 0, 0, 0];
    vp8x.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
    vp8x.extend_from_slice(&(height - 1).to_le_bytes()[..3]);
    let mut all: Vec<([u8; 4], &[u8])> = vec![(*b"VP8X", &vp8x)];
    if let Some(profile) = icc {
        all.push((*b"ICCP", profile));
    }
    all.extend(chunks);
    if let Some(block) = exif {
        all.push((*b"EXIF", block));
    }
    let mut body = b"WEBP".to_vec();
    append_chunks(&mut body, &all);
    Ok(riff(body))
}

fn append_chunks(body: &mut Vec<u8>, chunks: &[([u8; 4], &[u8])]) {
    for (id, data) in chunks {
        body.extend_from_slice(id);
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
        if data.len() % 2 == 1 {
            body.push(0);
        }
    }
}

fn riff(body: Vec<u8>) -> Vec<u8> {
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

/// Adds the ICC profile and the kept tags to a single-image little-endian TIFF: a new IFD0 (the
/// encoder's entries, Make, Model, Orientation, DateTime, the ICC profile and the Exif and GPS
/// pointers) and the Exif and GPS IFDs are appended, and the header points at the new IFD0. Strip
/// data and the encoder's out-of-line values stay where they are, so their offsets stay valid.
pub(crate) fn tiff_with(
    tiff: &[u8],
    kept: &Kept,
    icc: Option<&[u8]>,
    output: Output,
) -> Result<Vec<u8>, ContainerError> {
    if kept.is_empty() && icc.is_none() {
        return Ok(tiff.to_vec());
    }
    let malformed = ContainerError("malformed TIFF from the encoder");
    if tiff.len() < 8 || &tiff[..4] != b"II*\0" {
        return Err(ContainerError("only little-endian classic TIFF is handled"));
    }
    let word = |at: usize| {
        tiff.get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let dword = |at: usize| {
        tiff.get(at..at + 4)
            .map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")))
    };
    let first = dword(4).ok_or(malformed.clone())? as usize;
    let count = usize::from(word(first).ok_or(malformed.clone())?);
    let mut encoder_entries = Vec::with_capacity(count);
    for i in 0..count {
        let e = tiff
            .get(first + 2 + i * 12..first + 14 + i * 12)
            .ok_or(malformed.clone())?;
        let tag = u16::from_le_bytes([e[0], e[1]]);
        // The writer's own tags and any metadata the encoder might add (XMP, IPTC).
        if IFD0_ALLOWED.contains(&tag)
            || matches!(
                tag,
                TAG_EXIF_IFD | TAG_GPS_IFD | TAG_ICC_PROFILE | 0x02BC | 0x83BB
            )
        {
            continue;
        }
        encoder_entries.push(Entry {
            tag,
            kind: u16::from_le_bytes([e[2], e[3]]),
            count: u32::from_le_bytes(e[4..8].try_into().expect("4 bytes")),
            data: e[8..12].to_vec(),
        });
    }
    let verbatim: Vec<u16> = encoder_entries.iter().map(|e| e.tag).collect();
    let mut ifd0 = encoder_entries;
    ifd0.extend(kept.ifd0.iter().cloned());
    if let Some(profile) = icc {
        ifd0.push(undefined(TAG_ICC_PROFILE, profile));
    }
    let has_exif = !kept.is_empty();
    if has_exif {
        ifd0.push(pointer(TAG_EXIF_IFD));
    }
    if !kept.gps.is_empty() {
        ifd0.push(pointer(TAG_GPS_IFD));
    }

    let mut out = tiff.to_vec();
    let mut w = IfdWriter {
        out: &mut out,
        base: 0,
    };
    let (ifd0_at, slots) = w.write(&ifd0, &verbatim);
    if has_exif {
        let (exif_at, _) = w.write(&exif_ifd(kept, output), &[]);
        w.patch(&slots, TAG_EXIF_IFD, exif_at);
    }
    if !kept.gps.is_empty() {
        let (gps_at, _) = w.write(&kept.gps, &[]);
        w.patch(&slots, TAG_GPS_IFD, gps_at);
    }
    out[4..8].copy_from_slice(&ifd0_at.to_le_bytes());
    Ok(out)
}
