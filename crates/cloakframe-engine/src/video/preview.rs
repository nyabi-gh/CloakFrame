//! What review shows of a video analyzed for it (plan 4.4): source frame k as the passes see it,
//! the frame's tracked regions and manual boxes, and the frame masked as the second pass masks
//! it. Opening a preview checks the source's identity and timeline, not its bytes: nothing is
//! published from it, and the render that is checks them.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use cloakframe_core::geometry::Rect;
use cloakframe_core::item::ItemId;
use cloakframe_core::mask::{MaskStyle, MosaicGrid, Region};
use cloakframe_core::review::{EditError, ManualBox, VideoEdits};
use cloakframe_core::tracking::TrackId;

use super::process::{
    Masks, VideoError, VideoProcessor, item_records_dir, mask_frame, track_grids,
};
use super::record::{Frames, Record, RecordError, TrackRegion};
use super::tracking::TrackSummary;
use crate::cancel::CancelToken;
use crate::detect::Rgb8Image;
use crate::identity::SourceStamp;
use crate::media::decode::{self, DecodeError, FrameReader, frame_size};
use crate::media::source::{self, OpenError, Source};
use crate::photo::mask::ApplyError;
use crate::run::Analysis;

#[derive(Debug)]
pub enum PreviewError {
    /// The analysis holds no video, or no source stamp.
    NotAnalyzed,
    /// The source is not the analyzed file, or no longer has its timeline.
    ChangedSinceAnalysis,
    NoSuchFrame(u64),
    /// Edits the item's render would refuse; a preview of them would show what is not made.
    Edits(EditError),
    Open(OpenError),
    Records(RecordError),
    Decode(DecodeError),
    Mask(ApplyError),
    Settings(VideoError),
}

impl fmt::Display for PreviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnalyzed => VideoError::NotAnalyzed.fmt(f),
            Self::ChangedSinceAnalysis => VideoError::ChangedSinceAnalysis.fmt(f),
            Self::NoSuchFrame(k) => write!(f, "the video has no frame {k}"),
            Self::Edits(e) => e.fmt(f),
            Self::Open(e) => e.fmt(f),
            Self::Records(e) => write!(f, "the tracked regions cannot be read: {e}"),
            Self::Decode(e) => e.fmt(f),
            Self::Mask(e) => e.fmt(f),
            Self::Settings(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for PreviewError {}

/// A frame's regions as review lists them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FrameRegions {
    /// The tracked regions, those of excluded tracks too, in display pixels.
    pub tracked: Vec<TrackRegion>,
    /// The boxes of the manual tracks that show in the frame, each with its index in the edits.
    pub manual: Vec<(usize, ManualBox)>,
}

pub struct Preview {
    ffmpeg: PathBuf,
    input: PathBuf,
    source: Source,
    regions: Option<Record<TrackRegion>>,
    grids: BTreeMap<TrackId, MosaicGrid>,
    fixed: Vec<Region>,
    style: MaskStyle,
    tracks: Vec<TrackSummary>,
}

impl VideoProcessor {
    /// The preview of `item`'s video, analyzed by this processor's settings into its records.
    pub fn preview(
        &self,
        item: ItemId,
        input: &Path,
        analysis: &Analysis,
        cancel: &CancelToken,
    ) -> Result<Preview, PreviewError> {
        let (Some(stamp), Some(video)) = (analysis.source, analysis.video.as_ref()) else {
            return Err(PreviewError::NotAnalyzed);
        };
        let same = || SourceStamp::of(input).ok() == Some(stamp);
        if !same() {
            return Err(PreviewError::ChangedSinceAnalysis);
        }
        let source =
            source::open(&self.tools.ffprobe, input, cancel).map_err(PreviewError::Open)?;
        if !same() || source.timeline_digest() != video.timeline {
            return Err(PreviewError::ChangedSinceAnalysis);
        }
        let size = source.spec.display_size();
        let fixed = self.regions(size).map_err(PreviewError::Settings)?;
        let style = self.settings.style;
        let (regions, grids) = if video.tracks.is_empty() {
            (None, BTreeMap::new())
        } else {
            let records = item_records_dir(&self.records, item);
            let record = Record::open(&records.join("regions")).map_err(PreviewError::Records)?;
            let grids = track_grids(&record, &style, size).map_err(PreviewError::Records)?;
            (Some(record), grids)
        };
        Ok(Preview {
            ffmpeg: self.tools.ffmpeg.clone(),
            input: input.to_path_buf(),
            source,
            regions,
            grids,
            fixed,
            style,
            tracks: video.tracks.clone(),
        })
    }
}

impl Preview {
    /// Source frames, numbered from 0 as the records, gaps and edits number them.
    pub fn frames(&self) -> u64 {
        self.source.frames.len() as u64
    }

    /// The display size, the size regions and edits are in.
    pub fn size(&self) -> (u32, u32) {
        self.source.spec.display_size()
    }

    /// When frame `k` shows, in seconds after the first frame; playback keeps to it.
    pub fn seconds(&self, k: u64) -> f64 {
        let frames = &self.source.frames;
        frames.time(k as usize).as_f64() - frames.time(0).as_f64()
    }

    pub fn tracks(&self) -> &[TrackSummary] {
        &self.tracks
    }

    fn check(&self, k: u64) -> Result<(), PreviewError> {
        if k < self.frames() {
            Ok(())
        } else {
            Err(PreviewError::NoSuchFrame(k))
        }
    }

    /// Source frame `k`, the very frame the passes decode as their `k`-th (P-3), scaled to
    /// `long_edge` (the display size if `None`).
    pub fn frame(
        &self,
        k: u64,
        long_edge: Option<u32>,
        cancel: &CancelToken,
    ) -> Result<Rgb8Image, PreviewError> {
        self.check(k)?;
        let Source { spec, frames } = &self.source;
        let size = frame_size(spec, long_edge);
        decode::source_frame(
            &self.ffmpeg,
            &self.input,
            spec,
            frames,
            k as usize,
            size,
            cancel,
        )
        .map_err(PreviewError::Decode)
    }

    fn tracked(&self, k: u64) -> Result<Vec<TrackRegion>, PreviewError> {
        let mut entries = Vec::new();
        if let Some(record) = &self.regions {
            let mut frames = record
                .ascending_from(k)
                .map_err(PreviewError::Records)?
                .ok_or(PreviewError::NoSuchFrame(k))?;
            frames
                .next_into(&mut entries)
                .map_err(PreviewError::Records)?;
        }
        Ok(entries)
    }

    /// The render's own check of the edits (docs/M0_M5_REVIEW.md, item 11): a manual track
    /// with unordered keyframes has no box to show.
    fn check_edits(&self, edits: &VideoEdits) -> Result<(), PreviewError> {
        edits
            .check(
                |id| self.tracks.iter().any(|t| t.id == id),
                self.frames(),
                self.size(),
            )
            .map_err(PreviewError::Edits)
    }

    pub fn regions(&self, k: u64, edits: &VideoEdits) -> Result<FrameRegions, PreviewError> {
        self.check(k)?;
        self.check_edits(edits)?;
        let size = self.size();
        Ok(FrameRegions {
            tracked: self.tracked(k)?,
            manual: edits
                .added_tracks
                .iter()
                .enumerate()
                .filter_map(|(i, track)| track.box_at(k, size).map(|b| (i, b)))
                .collect(),
        })
    }

    /// Frame `k` masked as the second pass masks it under `edits`. At the display size the
    /// masks are the second pass's own; smaller, the regions and mosaic cells scale with it.
    pub fn masked(
        &self,
        k: u64,
        edits: &VideoEdits,
        long_edge: Option<u32>,
        cancel: &CancelToken,
    ) -> Result<Rgb8Image, PreviewError> {
        self.check_edits(edits)?;
        let mut frame = self.frame(k, long_edge, cancel)?;
        let entries = self.tracked(k)?;
        let masks = self.masks(edits);
        let mut regions = Vec::new();
        masks.frame(k, &entries, &mut regions);
        self.mask(&mut frame, &mut regions)?;
        Ok(frame)
    }

    fn masks<'a>(&'a self, edits: &'a VideoEdits) -> Masks<'a> {
        let tracked = self.regions.as_ref().map(|r| (r, self.grids.clone()));
        Masks::new(self.fixed.clone(), tracked, edits, &self.style, self.size())
    }

    /// Masks `frame` with display-size `regions`, scaled with the frame.
    fn mask(&self, frame: &mut Rgb8Image, regions: &mut [Region]) -> Result<(), PreviewError> {
        let size = self.size();
        if (frame.width, frame.height) != size {
            let sx = frame.width as f32 / size.0 as f32;
            let sy = frame.height as f32 / size.1 as f32;
            for r in regions.iter_mut() {
                let b = r.bbox;
                r.bbox = Rect::new(b.x() * sx, b.y() * sy, b.width() * sx, b.height() * sy)
                    .expect("a box scaled by a positive factor");
                r.grid = r
                    .grid
                    .map(|g| MosaicGrid::new((g.cell() as f32 * sx).round() as u32));
            }
        }
        mask_frame(frame, regions, &self.style).map_err(PreviewError::Mask)
    }

    /// Frames `from..` one after another at `long_edge`, masked under `edits` if given, for
    /// playback: one decoder for them all rather than a seek for each.
    pub fn play<'a>(
        &'a self,
        from: u64,
        edits: Option<&'a VideoEdits>,
        long_edge: Option<u32>,
        cancel: &CancelToken,
    ) -> Result<Playback<'a>, PreviewError> {
        self.check(from)?;
        if let Some(edits) = edits {
            self.check_edits(edits)?;
        }
        let Source { spec, frames } = &self.source;
        let reader = FrameReader::start_at(
            &self.ffmpeg,
            &self.input,
            spec,
            frames,
            from as usize,
            frame_size(spec, long_edge),
            cancel,
        )
        .map_err(PreviewError::Decode)?;
        let tracked = match (edits, &self.regions) {
            (Some(_), Some(record)) => Some(
                record
                    .ascending_from(from)
                    .map_err(PreviewError::Records)?
                    .ok_or(PreviewError::NoSuchFrame(from))?,
            ),
            _ => None,
        };
        Ok(Playback {
            preview: self,
            reader,
            tracked,
            masks: edits.map(|e| self.masks(e)),
            next: from,
            entries: Vec::new(),
            regions: Vec::new(),
        })
    }
}

/// [`Preview::play`]'s frames.
pub struct Playback<'a> {
    preview: &'a Preview,
    reader: FrameReader,
    tracked: Option<Frames<TrackRegion>>,
    masks: Option<Masks<'a>>,
    next: u64,
    entries: Vec<TrackRegion>,
    regions: Vec<Region>,
}

impl Playback<'_> {
    /// The next frame and its number; `None` after the last.
    pub fn next_frame(&mut self) -> Result<Option<(u64, Rgb8Image)>, PreviewError> {
        let mut frame = Rgb8Image {
            width: 0,
            height: 0,
            data: Vec::new(),
        };
        if !self
            .reader
            .next_into(&mut frame)
            .map_err(PreviewError::Decode)?
        {
            return Ok(None);
        }
        let k = self.next;
        self.next += 1;
        if let Some(masks) = &self.masks {
            self.entries.clear();
            if let Some(tracked) = &mut self.tracked {
                let read = tracked
                    .next_into(&mut self.entries)
                    .map_err(PreviewError::Records)?;
                if read != Some(k) {
                    return Err(PreviewError::Records(RecordError::Damaged(
                        "missing frames",
                    )));
                }
            }
            masks.frame(k, &self.entries, &mut self.regions);
            self.preview.mask(&mut frame, &mut self.regions)?;
        }
        Ok(Some((k, frame)))
    }
}
