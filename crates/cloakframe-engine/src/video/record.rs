//! The files a video's analysis keeps on disk instead of in memory (plan 4.4): what was detected
//! in every frame, and the regions tracking made of it. Frames are appended one block at a time
//! and read in either direction, so tracking holds only the tracks still open however long the
//! video is.
//!
//! A file is a header, one block per frame, and a footer:
//!
//! - header: `CLOAKREC`, the format version (u16), what the entries are (u8), whether frames were
//!   appended in ascending or descending order (u8), 4 bytes zero;
//! - block: frame (u32), entry count (u32), the entries, the block's length in bytes (u32), which
//!   lets a reader step back from a block's end;
//! - footer: the offset of every [`INDEX_EVERY`]th block in append order (u64 each), the scene cuts
//!   (u64 each), the number of index offsets, of cuts and of frames (u32 each), `RECORDEND`.
//!
//! Numbers are little-endian. A file without its footer was not finished and is not read.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use cloakframe_core::detection::{DetectionKind, FrameIndex, MAX_ROLL, ModelId};
use cloakframe_core::geometry::Rect;
use cloakframe_core::tracking::tracker::Assignment;
use cloakframe_core::tracking::{Observation, TrackId};

pub const VERSION: u16 = 1;
const MAGIC: &[u8; 8] = b"CLOAKREC";
const END: &[u8; 9] = b"RECORDEND";
const HEADER: u64 = 16;
const BLOCK_HEAD: usize = 8;
const BLOCK_TAIL: usize = 4;
const FOOTER_TAIL: u64 = 12 + END.len() as u64;
/// One index offset per this many blocks: an hour at 60 frames per second is 211 offsets.
pub const INDEX_EVERY: u64 = 1024;
/// How much a reader takes from the file at a time.
const CHUNK: usize = 64 * 1024;

#[derive(Debug)]
pub enum RecordError {
    Io(io::Error),
    UnsupportedVersion(u16),
    /// Not a record of the expected entries, cut short, or changed.
    Damaged(&'static str),
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read the analysis record: {e}"),
            Self::UnsupportedVersion(v) => write!(f, "analysis record format {v} is not supported"),
            Self::Damaged(why) => write!(f, "the analysis record is damaged: {why}"),
        }
    }
}

impl std::error::Error for RecordError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for RecordError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// What a record holds for each frame, at a fixed size.
pub trait Entry: Sized {
    /// Written to the header, so a file of other entries is refused.
    const CONTENTS: u8;
    const SIZE: usize;
    fn put(&self, out: &mut Vec<u8>);
    /// `None` when the bytes are not an entry this writes.
    fn get(bytes: &[u8]) -> Option<Self>;
}

fn f32_at(bytes: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"))
}

fn put_rect(out: &mut Vec<u8>, r: &Rect) {
    for v in [r.x(), r.y(), r.width(), r.height()] {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

fn get_rect(bytes: &[u8], at: usize) -> Option<Rect> {
    Rect::new(
        f32_at(bytes, at),
        f32_at(bytes, at + 4),
        f32_at(bytes, at + 8),
        f32_at(bytes, at + 12),
    )
    .ok()
}

/// A missing roll is stored as NaN.
fn put_roll(out: &mut Vec<u8>, roll: Option<f32>) {
    out.extend_from_slice(&roll.unwrap_or(f32::NAN).to_le_bytes());
}

fn get_roll(bytes: &[u8], at: usize) -> Option<Option<f32>> {
    let roll = f32_at(bytes, at);
    if roll.is_nan() {
        Some(None)
    } else if roll.abs() <= MAX_ROLL {
        Some(Some(roll))
    } else {
        None
    }
}

fn get_score(bytes: &[u8], at: usize) -> Option<f32> {
    let score = f32_at(bytes, at);
    (0.0..=1.0).contains(&score).then_some(score)
}

fn model_code(model: ModelId) -> u8 {
    match model {
        ModelId::Yolo5FaceN => 1,
        ModelId::YuNet => 2,
        ModelId::PlateYolov9T => 3,
    }
}

fn model_of(code: u8) -> Option<ModelId> {
    match code {
        1 => Some(ModelId::Yolo5FaceN),
        2 => Some(ModelId::YuNet),
        3 => Some(ModelId::PlateYolov9T),
        _ => None,
    }
}

fn kind_code(kind: DetectionKind) -> u8 {
    match kind {
        DetectionKind::Face => 1,
        DetectionKind::Plate => 2,
    }
}

fn kind_of(code: u8) -> Option<DetectionKind> {
    match code {
        1 => Some(DetectionKind::Face),
        2 => Some(DetectionKind::Plate),
        _ => None,
    }
}

/// One detection in source display pixels, with a face's roll instead of its landmarks: masks
/// need nothing else of them (docs/M5_PLAN.md).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Detected {
    pub model: ModelId,
    pub observation: Observation,
}

impl Entry for Detected {
    const CONTENTS: u8 = 1;
    const SIZE: usize = 25;

    fn put(&self, out: &mut Vec<u8>) {
        let o = &self.observation;
        out.push(model_code(self.model));
        out.extend_from_slice(&o.score.to_le_bytes());
        put_rect(out, &o.bbox);
        put_roll(out, o.roll);
    }

    fn get(bytes: &[u8]) -> Option<Self> {
        Some(Self {
            model: model_of(bytes[0])?,
            observation: Observation {
                score: get_score(bytes, 1)?,
                bbox: get_rect(bytes, 5)?,
                roll: get_roll(bytes, 21)?,
            },
        })
    }
}

/// Which detection of its kind continued or started which track in one direction of tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Assigned {
    pub kind: DetectionKind,
    pub assignment: Assignment,
}

impl Entry for Assigned {
    const CONTENTS: u8 = 2;
    const SIZE: usize = 10;

    fn put(&self, out: &mut Vec<u8>) {
        let a = &self.assignment;
        out.push(kind_code(self.kind));
        out.extend_from_slice(&(a.detection as u32).to_le_bytes());
        out.extend_from_slice(&a.track.to_le_bytes());
        out.push(u8::from(a.started));
    }

    fn get(bytes: &[u8]) -> Option<Self> {
        Some(Self {
            kind: kind_of(bytes[0])?,
            assignment: Assignment {
                detection: u32_at(bytes, 1) as usize,
                track: u32_at(bytes, 5),
                started: match bytes[9] {
                    0 => false,
                    1 => true,
                    _ => return None,
                },
            },
        })
    }
}

/// A region to mask in one frame: a track's box after post-processing, in source display pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackRegion {
    pub kind: DetectionKind,
    pub track: TrackId,
    pub bbox: Rect,
    pub score: f32,
    pub roll: Option<f32>,
    /// Filled in by tracking rather than seen by the detector.
    pub interpolated: bool,
}

impl Entry for TrackRegion {
    const CONTENTS: u8 = 3;
    const SIZE: usize = 30;

    fn put(&self, out: &mut Vec<u8>) {
        out.push(kind_code(self.kind));
        out.push(u8::from(self.interpolated));
        out.extend_from_slice(&self.track.to_le_bytes());
        out.extend_from_slice(&self.score.to_le_bytes());
        put_rect(out, &self.bbox);
        put_roll(out, self.roll);
    }

    fn get(bytes: &[u8]) -> Option<Self> {
        Some(Self {
            kind: kind_of(bytes[0])?,
            interpolated: match bytes[1] {
                0 => false,
                1 => true,
                _ => return None,
            },
            track: u32_at(bytes, 2),
            score: get_score(bytes, 6)?,
            bbox: get_rect(bytes, 10)?,
            roll: get_roll(bytes, 26)?,
        })
    }
}

/// The order frames are appended in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Ascending,
    /// For what backward tracking finds, which reading the file from its end turns into frame
    /// order.
    Descending,
}

pub struct RecordWriter<E: Entry> {
    file: BufWriter<File>,
    order: Order,
    offset: u64,
    next: Option<FrameIndex>,
    frames: u32,
    index: Vec<u64>,
    block: Vec<u8>,
    _entries: PhantomData<E>,
}

impl<E: Entry> RecordWriter<E> {
    /// A new file at `path`, which must not exist.
    pub fn create(path: &Path, order: Order) -> io::Result<Self> {
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let mut file = BufWriter::with_capacity(CHUNK, file);
        let mut header = [0u8; HEADER as usize];
        header[..8].copy_from_slice(MAGIC);
        header[8..10].copy_from_slice(&VERSION.to_le_bytes());
        header[10] = E::CONTENTS;
        header[11] = match order {
            Order::Ascending => 0,
            Order::Descending => 1,
        };
        file.write_all(&header)?;
        Ok(Self {
            file,
            order,
            offset: HEADER,
            next: None,
            frames: 0,
            index: Vec::new(),
            block: Vec::new(),
            _entries: PhantomData,
        })
    }

    /// Appends `frame`, which follows the frame before in the file's order: one up, or one down.
    pub fn frame(&mut self, frame: FrameIndex, entries: &[E]) -> io::Result<()> {
        if let Some(next) = self.next {
            assert_eq!(frame, next, "frames appended one after another");
        }
        let number = u32::try_from(frame).map_err(|_| too_large("frame"))?;
        let count = u32::try_from(entries.len()).map_err(|_| too_large("frame's entries"))?;
        let length = BLOCK_HEAD + entries.len() * E::SIZE + BLOCK_TAIL;
        let length = u32::try_from(length).map_err(|_| too_large("frame's entries"))?;
        if u64::from(self.frames) % INDEX_EVERY == 0 {
            self.index.push(self.offset);
        }
        self.block.clear();
        self.block.extend_from_slice(&number.to_le_bytes());
        self.block.extend_from_slice(&count.to_le_bytes());
        for e in entries {
            e.put(&mut self.block);
        }
        self.block.extend_from_slice(&length.to_le_bytes());
        debug_assert_eq!(self.block.len(), length as usize);
        self.file.write_all(&self.block)?;
        self.offset += u64::from(length);
        self.frames = self
            .frames
            .checked_add(1)
            .ok_or_else(|| too_large("frames"))?;
        self.next = match self.order {
            Order::Ascending => Some(frame + 1),
            Order::Descending => frame.checked_sub(1),
        };
        Ok(())
    }

    /// Writes the footer, which makes the file readable. A descending file ends at frame 0.
    pub fn finish(mut self, cuts: &[FrameIndex]) -> io::Result<()> {
        if self.order == Order::Descending && self.frames > 0 {
            assert_eq!(self.next, None, "a descending record ends at frame 0");
        }
        let mut footer = Vec::with_capacity((self.index.len() + cuts.len()) * 8 + 21);
        for offset in &self.index {
            footer.extend_from_slice(&offset.to_le_bytes());
        }
        for cut in cuts {
            footer.extend_from_slice(&cut.to_le_bytes());
        }
        let cut_count = u32::try_from(cuts.len()).map_err(|_| too_large("cuts"))?;
        footer.extend_from_slice(&(self.index.len() as u32).to_le_bytes());
        footer.extend_from_slice(&cut_count.to_le_bytes());
        footer.extend_from_slice(&self.frames.to_le_bytes());
        footer.extend_from_slice(END);
        self.file.write_all(&footer)?;
        self.file.flush()
    }
}

fn too_large(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("too many {what} for an analysis record"),
    )
}

/// A finished record. Readers of it can run one after another or side by side.
pub struct Record<E: Entry> {
    path: PathBuf,
    order: Order,
    frames: u64,
    cuts: Vec<FrameIndex>,
    index: Vec<u64>,
    data_end: u64,
    _entries: PhantomData<E>,
}

fn damaged(why: &'static str) -> RecordError {
    RecordError::Damaged(why)
}

fn read_at(file: &mut File, offset: u64, buf: &mut [u8]) -> io::Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(buf)
}

impl<E: Entry> Record<E> {
    pub fn open(path: &Path) -> Result<Self, RecordError> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        if len < HEADER + FOOTER_TAIL {
            return Err(damaged("too short"));
        }
        let mut header = [0u8; HEADER as usize];
        read_at(&mut file, 0, &mut header)?;
        if &header[..8] != MAGIC {
            return Err(damaged("not an analysis record"));
        }
        let version = u16::from_le_bytes([header[8], header[9]]);
        if version != VERSION {
            return Err(RecordError::UnsupportedVersion(version));
        }
        if header[10] != E::CONTENTS {
            return Err(damaged("holds other entries"));
        }
        let order = match header[11] {
            0 => Order::Ascending,
            1 => Order::Descending,
            _ => return Err(damaged("unknown order")),
        };
        let mut tail = [0u8; FOOTER_TAIL as usize];
        read_at(&mut file, len - FOOTER_TAIL, &mut tail)?;
        if &tail[12..] != END {
            return Err(damaged("not finished"));
        }
        let (index_count, cut_count) = (u32_at(&tail, 0) as u64, u32_at(&tail, 4) as u64);
        let frames = u64::from(u32_at(&tail, 8));
        let lists = (index_count + cut_count) * 8;
        if index_count != frames.div_ceil(INDEX_EVERY) || len < HEADER + FOOTER_TAIL + lists {
            return Err(damaged("footer"));
        }
        let data_end = len - FOOTER_TAIL - lists;
        let mut bytes = vec![0u8; lists as usize];
        read_at(&mut file, data_end, &mut bytes)?;
        let index: Vec<u64> = (0..index_count as usize)
            .map(|i| u64_at(&bytes, i * 8))
            .collect();
        let cuts: Vec<FrameIndex> = (index_count as usize..(index_count + cut_count) as usize)
            .map(|i| u64_at(&bytes, i * 8))
            .collect();
        let ascending = index.windows(2).all(|w| w[0] < w[1]);
        if !ascending || index.first().is_some_and(|&o| o != HEADER) {
            return Err(damaged("index"));
        }
        if index.last().is_some_and(|&o| o >= data_end) || (frames == 0) != (data_end == HEADER) {
            return Err(damaged("index"));
        }
        if !cuts.windows(2).all(|w| w[0] < w[1]) || cuts.iter().any(|&c| c == 0 || c >= frames) {
            return Err(damaged("cuts"));
        }
        Ok(Self {
            path: path.to_owned(),
            order,
            frames,
            cuts,
            index,
            data_end,
            _entries: PhantomData,
        })
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn cuts(&self) -> &[FrameIndex] {
        &self.cuts
    }

    /// Every frame from 0 up.
    pub fn ascending(&self) -> Result<Frames<E>, RecordError> {
        self.frames_from(self.order == Order::Ascending, HEADER, self.data_end)
    }

    /// Every frame from the last down.
    pub fn descending(&self) -> Result<Frames<E>, RecordError> {
        self.frames_from(self.order == Order::Descending, HEADER, self.data_end)
    }

    /// The frames from `frame` up, found through the index, of a record appended in ascending
    /// order. `None` past the last frame.
    pub fn ascending_from(&self, frame: FrameIndex) -> Result<Option<Frames<E>>, RecordError> {
        assert_eq!(self.order, Order::Ascending, "seeking an ascending record");
        if frame >= self.frames {
            return Ok(None);
        }
        let stretch = frame / INDEX_EVERY;
        let mut frames = self.frames_from(true, self.index[stretch as usize], self.data_end)?;
        frames.expected = Some(stretch * INDEX_EVERY);
        let mut skipped = Vec::new();
        while frames.expected.is_some_and(|f| f < frame) {
            frames.next_into(&mut skipped)?;
        }
        Ok(Some(frames))
    }

    fn frames_from(&self, forwards: bool, start: u64, end: u64) -> Result<Frames<E>, RecordError> {
        let file = File::open(&self.path)?;
        let ascending_in_file = self.order == Order::Ascending;
        let expected = match (forwards, ascending_in_file) {
            _ if self.frames == 0 => None,
            (true, true) => Some(0),
            (false, false) => Some(0),
            (true, false) | (false, true) => Some(self.frames - 1),
        };
        Ok(Frames {
            file,
            forwards,
            start,
            end,
            pos: if forwards { start } else { end },
            window: Vec::new(),
            window_start: 0,
            expected,
            ascending: forwards == ascending_in_file,
            frames: self.frames,
            _entries: PhantomData,
        })
    }
}

/// Frames of a record one at a time, in one direction.
pub struct Frames<E: Entry> {
    file: File,
    /// Reading from the start of the file to its end.
    forwards: bool,
    start: u64,
    end: u64,
    pos: u64,
    window: Vec<u8>,
    window_start: u64,
    expected: Option<FrameIndex>,
    /// Frame numbers go up.
    ascending: bool,
    frames: u64,
    _entries: PhantomData<E>,
}

impl<E: Entry> Frames<E> {
    /// Makes `[from, from + len)` of the file available in the window and returns where it
    /// starts there.
    fn bytes(&mut self, from: u64, len: usize) -> Result<usize, RecordError> {
        let window_end = self.window_start + self.window.len() as u64;
        if from >= self.window_start && from + len as u64 <= window_end {
            return Ok((from - self.window_start) as usize);
        }
        let take = len.max(CHUNK) as u64;
        let (lo, hi) = if self.forwards {
            (from, (from + take).min(self.end))
        } else {
            let hi = from + len as u64;
            (hi.saturating_sub(take).max(self.start), hi)
        };
        if lo < self.start || hi > self.end || hi - lo < len as u64 {
            return Err(damaged("block outside the frames"));
        }
        self.window.resize((hi - lo) as usize, 0);
        read_at(&mut self.file, lo, &mut self.window)?;
        self.window_start = lo;
        Ok((from - lo) as usize)
    }

    /// The next frame's entries into `entries`, replacing what it held, and its number; `None`
    /// after the last.
    pub fn next_into(&mut self, entries: &mut Vec<E>) -> Result<Option<FrameIndex>, RecordError> {
        entries.clear();
        let Some(expected) = self.expected else {
            return Ok(None);
        };
        let (head, length) = if self.forwards {
            if self.pos + (BLOCK_HEAD + BLOCK_TAIL) as u64 > self.end {
                return Err(damaged("missing frames"));
            }
            let at = self.bytes(self.pos, BLOCK_HEAD)?;
            let count = u32_at(&self.window, at + 4) as u64;
            (
                self.pos,
                (BLOCK_HEAD + BLOCK_TAIL) as u64 + count * E::SIZE as u64,
            )
        } else {
            if self.pos < self.start + (BLOCK_HEAD + BLOCK_TAIL) as u64 {
                return Err(damaged("missing frames"));
            }
            let at = self.bytes(self.pos - BLOCK_TAIL as u64, BLOCK_TAIL)?;
            let length = u64::from(u32_at(&self.window, at));
            let head = self
                .pos
                .checked_sub(length)
                .ok_or_else(|| damaged("block length"))?;
            (head, length)
        };
        let length_usize = usize::try_from(length).map_err(|_| damaged("block length"))?;
        let at = self.bytes(head, length_usize)?;
        let block = &self.window[at..at + length_usize];
        let frame = u64::from(u32_at(block, 0));
        let count = u32_at(block, 4) as usize;
        let tail = u32_at(block, length_usize - BLOCK_TAIL) as u64;
        if frame != expected
            || tail != length
            || BLOCK_HEAD + count * E::SIZE + BLOCK_TAIL != length_usize
        {
            return Err(damaged("block"));
        }
        for i in 0..count {
            let from = BLOCK_HEAD + i * E::SIZE;
            let entry = E::get(&block[from..from + E::SIZE]).ok_or_else(|| damaged("entry"))?;
            entries.push(entry);
        }
        if self.forwards {
            self.pos += length;
        } else {
            self.pos = head;
        }
        self.expected = if self.ascending {
            (frame + 1 < self.frames).then_some(frame + 1)
        } else {
            frame.checked_sub(1)
        };
        Ok(Some(frame))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;

    fn face(frame: FrameIndex, i: usize) -> Detected {
        Detected {
            model: ModelId::Yolo5FaceN,
            observation: Observation {
                bbox: Rect::new(frame as f32, i as f32, 10.0 + i as f32, 12.0).unwrap(),
                score: 0.1 + 0.1 * i as f32,
                roll: i.is_multiple_of(2).then_some(-0.5),
            },
        }
    }

    /// A different number of detections in each frame, some none.
    fn detections(frame: FrameIndex) -> Vec<Detected> {
        (0..(frame % 4) as usize).map(|i| face(frame, i)).collect()
    }

    fn write(path: &Path, frames: FrameIndex, order: Order, cuts: &[FrameIndex]) {
        let mut writer = RecordWriter::<Detected>::create(path, order).unwrap();
        let numbers: Vec<FrameIndex> = match order {
            Order::Ascending => (0..frames).collect(),
            Order::Descending => (0..frames).rev().collect(),
        };
        for k in numbers {
            writer.frame(k, &detections(k)).unwrap();
        }
        writer.finish(cuts).unwrap();
    }

    fn read_all(mut frames: Frames<Detected>) -> Vec<(FrameIndex, Vec<Detected>)> {
        let mut out = Vec::new();
        let mut entries = Vec::new();
        while let Some(k) = frames.next_into(&mut entries).unwrap() {
            out.push((k, entries.clone()));
        }
        out
    }

    fn expected(range: impl Iterator<Item = FrameIndex>) -> Vec<(FrameIndex, Vec<Detected>)> {
        range.map(|k| (k, detections(k))).collect()
    }

    #[test]
    fn frames_read_back_in_either_direction_whichever_order_they_were_written_in() {
        let dir = TestDir::new();
        // Past two index stretches, with the reader's window smaller than the file.
        let n = 2 * INDEX_EVERY + 5;
        for order in [Order::Ascending, Order::Descending] {
            let path = dir.path().join(format!("{order:?}"));
            write(&path, n, order, &[7, 1500]);
            let record = Record::<Detected>::open(&path).unwrap();
            assert_eq!((record.frames(), record.cuts()), (n, &[7, 1500][..]));
            assert_eq!(read_all(record.ascending().unwrap()), expected(0..n));
            assert_eq!(
                read_all(record.descending().unwrap()),
                expected((0..n).rev())
            );
        }
    }

    #[test]
    fn reading_from_a_frame_starts_there() {
        let dir = TestDir::new();
        let path = dir.path().join("r");
        let n = 2 * INDEX_EVERY + 5;
        write(&path, n, Order::Ascending, &[]);
        let record = Record::<Detected>::open(&path).unwrap();
        for from in [0, 1, INDEX_EVERY - 1, INDEX_EVERY, INDEX_EVERY + 3, n - 1] {
            let frames = record.ascending_from(from).unwrap().unwrap();
            assert_eq!(read_all(frames), expected(from..n), "from {from}");
        }
        assert!(record.ascending_from(n).unwrap().is_none());
    }

    #[test]
    fn entries_take_their_stated_size() {
        let dir = TestDir::new();
        let path = dir.path().join("r");
        write(&path, 4, Order::Ascending, &[]);
        // Frames 1–3 hold 1, 2 and 3 detections; each block has a 12-byte frame head and tail.
        let blocks = 4 * 12 + 6 * Detected::SIZE as u64;
        let footer = 8 + 12 + END.len() as u64;
        assert_eq!(fs_len(&path), HEADER + blocks + footer);
        let mut bytes = Vec::new();
        let region = TrackRegion {
            kind: DetectionKind::Plate,
            track: 7,
            bbox: Rect::new(1.0, 2.0, 3.0, 4.0).unwrap(),
            score: 0.25,
            roll: None,
            interpolated: true,
        };
        region.put(&mut bytes);
        assert_eq!(
            (bytes.len(), TrackRegion::get(&bytes)),
            (TrackRegion::SIZE, Some(region))
        );
        bytes.clear();
        let assigned = Assigned {
            kind: DetectionKind::Face,
            assignment: Assignment {
                detection: 3,
                track: 9,
                started: true,
            },
        };
        assigned.put(&mut bytes);
        assert_eq!(
            (bytes.len(), Assigned::get(&bytes)),
            (Assigned::SIZE, Some(assigned))
        );
    }

    fn fs_len(path: &Path) -> u64 {
        std::fs::metadata(path).unwrap().len()
    }

    type Change = Box<dyn FnOnce(&mut Vec<u8>)>;

    fn damaged_after(change: impl FnOnce(&mut Vec<u8>)) -> RecordError {
        let dir = TestDir::new();
        let path = dir.path().join("r");
        write(&path, 40, Order::Ascending, &[3]);
        let mut bytes = std::fs::read(&path).unwrap();
        change(&mut bytes);
        std::fs::write(&path, &bytes).unwrap();
        match Record::<Detected>::open(&path) {
            Err(e) => e,
            Ok(record) => {
                let mut frames = record.ascending().unwrap();
                let mut entries = Vec::new();
                loop {
                    match frames.next_into(&mut entries) {
                        Ok(Some(_)) => {}
                        Ok(None) => panic!("read the changed record"),
                        Err(e) => return e,
                    }
                }
            }
        }
    }

    #[test]
    fn a_cut_short_or_changed_record_is_not_read() {
        let cases: Vec<(&str, Change)> = vec![
            ("cut short", Box::new(|b| b.truncate(b.len() - 30))),
            ("no footer", Box::new(|b| b.truncate(200))),
            ("other version", Box::new(|b| b[8] = 2)),
            ("other entries", Box::new(|b| b[10] = Assigned::CONTENTS)),
            (
                "unknown model",
                Box::new(|b| b[HEADER as usize + 12 + 8] = 9),
            ),
            (
                "score above 1",
                Box::new(|b| {
                    let at = HEADER as usize + 12 + 8 + 1;
                    b[at..at + 4].copy_from_slice(&2.0f32.to_le_bytes());
                }),
            ),
            ("frame number", Box::new(|b| b[HEADER as usize] = 5)),
        ];
        for (what, change) in cases {
            let e = damaged_after(change);
            assert!(
                matches!(
                    e,
                    RecordError::Damaged(_) | RecordError::UnsupportedVersion(2)
                ),
                "{what}: {e}"
            );
        }
        let dir = TestDir::new();
        let path = dir.path().join("r");
        write(&path, 3, Order::Ascending, &[]);
        assert!(matches!(
            Record::<TrackRegion>::open(&path),
            Err(RecordError::Damaged(_))
        ));
    }

    #[test]
    fn an_existing_file_is_not_replaced() {
        let dir = TestDir::new();
        let path = dir.path().join("r");
        std::fs::write(&path, b"x").unwrap();
        let e = RecordWriter::<Detected>::create(&path, Order::Ascending)
            .err()
            .unwrap();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn an_empty_video_has_an_empty_record() {
        let dir = TestDir::new();
        let path = dir.path().join("r");
        write(&path, 0, Order::Ascending, &[]);
        let record = Record::<Detected>::open(&path).unwrap();
        assert_eq!(record.frames(), 0);
        assert!(read_all(record.ascending().unwrap()).is_empty());
        assert!(read_all(record.descending().unwrap()).is_empty());
    }
}
