//! The video pipeline as the runner's item processor (plan 4.4). Analysis reads the timeline,
//! detects in every frame and tracks into the item's records; the second pass decodes every
//! source frame at full size, masks the frame's tracked regions and the fixed areas the user
//! gives, encodes, and checks the output (P-4) before it waits for its publish permit. The source
//! is never copied; its identity is compared before the first pass, after it, between the passes,
//! after the second and right before publishing (plan 0.1). Beside the first pass, and the second
//! when it resumes a review, a thread reads the source once more for the digest a review is bound
//! to; a review resumed from a session renders only the same bytes on the same timeline.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use cloakframe_core::detection::DetectionKind;
use cloakframe_core::geometry::Rect;
use cloakframe_core::item::{ItemId, PublishBasis, PublishPermit};
use cloakframe_core::mask::{MaskStyle, MosaicGrid, Region};
use cloakframe_core::review::{Digest, ManualTrack, VideoEdits};
use cloakframe_core::tracking::TrackId;
use image::{DynamicImage, RgbImage};
use sha2::{Digest as _, Sha256};

use super::analysis::{self, AnalysisError, AnalysisSettings};
use super::record::{Detected, Record, RecordError, TrackRegion};
use super::tracking::{self, KindTracking, TrackSummary, TrackingError};
use crate::cancel::CancelToken;
use crate::detect::Rgb8Image;
use crate::identity::{SourceStamp, file_digest};
use crate::media::decode::{DecodeError, FrameReader, frame_size};
use crate::media::encode::{
    Destination, EncodeError, Encoder, Encoders, FrameWriter, OutputCodec, Quality, encoded_size,
};
use crate::media::source::{self, Source};
use crate::media::verify::{self, Expected, VerifyError};
use crate::photo::mask::{self, ApplyError};
use crate::publish::TempOutput;
use crate::run::{
    Analysis, Classify, FailureKind, Fingerprint, ItemContext, ItemProcessor, Note, Notes,
    RenderRequest, StepError,
};

/// The bundled tools (plan 0.1: run as external processes).
#[derive(Debug, Clone)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoSettings {
    pub codec: OutputCodec,
    pub quality: Quality,
    /// Try the platform's hardware encoders before software (1.12.1's default).
    pub hardware_encode: bool,
    /// Leave the source's audio out of the output. Kept by default, as 1.12.1 did; voices and
    /// anything said in them are not anonymized.
    pub remove_audio: bool,
    /// Areas masked in every frame, in display pixels.
    pub masks: Vec<Rect>,
    pub style: MaskStyle,
    pub analysis: AnalysisSettings,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            codec: OutputCodec::H264,
            quality: Quality::High,
            hardware_encode: true,
            remove_audio: false,
            masks: Vec::new(),
            style: MaskStyle::default(),
            analysis: AnalysisSettings::default(),
        }
    }
}

impl VideoSettings {
    fn digest(&self) -> Digest {
        let s = &self.style;
        let mut h = Sha256::new();
        h.update(format!(
            "codec {:?} quality {:?} hardware {} audio removed {}\nstyle {:?} {:?} padding {:08x} soft {} mosaic {}\n",
            self.codec,
            self.quality,
            self.hardware_encode,
            self.remove_audio,
            s.method,
            s.shape,
            s.padding.ratio().to_bits(),
            s.soft_edges,
            s.mosaic_strength.cells(),
        ));
        h.update(self.analysis.digest().0);
        for m in &self.masks {
            h.update(format!(
                "mask {:08x} {:08x} {:08x} {:08x}\n",
                m.x().to_bits(),
                m.y().to_bits(),
                m.width().to_bits(),
                m.height().to_bits()
            ));
        }
        Digest(h.finalize().into())
    }
}

#[derive(Debug)]
pub enum VideoError {
    /// The source changed between reading its timeline and publishing the output.
    ChangedWhileProcessing,
    /// A review resumed from a session: the bytes or the timeline are not the analyzed ones.
    ChangedSinceAnalysis,
    /// A review resumed from a session that does not hold a video's analysis.
    NotAnalyzed,
    NameTaken,
    /// A mask that does not overlap the frame at all, most likely meant for another video.
    MaskOutside {
        mask: usize,
        size: (u32, u32),
    },
}

impl fmt::Display for VideoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChangedWhileProcessing => {
                f.write_str("the source changed while it was processed")
            }
            Self::ChangedSinceAnalysis => {
                f.write_str("the source changed since it was analyzed; run it again")
            }
            Self::NotAnalyzed => f.write_str("the item has no video analysis to resume"),
            Self::NameTaken => f.write_str("the output name was taken during the run"),
            Self::MaskOutside { mask, size } => write!(
                f,
                "mask {} lies outside the {}×{} video",
                mask + 1,
                size.0,
                size.1
            ),
        }
    }
}

impl std::error::Error for VideoError {}

impl Classify for VideoError {
    fn kind(&self) -> FailureKind {
        match self {
            Self::ChangedWhileProcessing | Self::ChangedSinceAnalysis => FailureKind::SourceChanged,
            Self::NameTaken => FailureKind::NameTaken,
            Self::NotAnalyzed | Self::MaskOutside { .. } => FailureKind::Other,
        }
    }
}

fn failed(e: impl Classify + std::error::Error + Send + Sync + 'static) -> StepError {
    e.into()
}

/// A tool stopped by the item's cancel reports its own error; the cancel is what happened.
fn failed_unless_cancelled(ctx: &ItemContext<'_>, e: impl Into<StepError>) -> StepError {
    if ctx.cancel.is_cancelled() {
        StepError::Cancelled
    } else {
        e.into()
    }
}

fn unchanged(ctx: &ItemContext<'_>, stamp: SourceStamp) -> Result<(), StepError> {
    if SourceStamp::of(ctx.input).ok() == Some(stamp) {
        Ok(())
    } else {
        Err(failed(VideoError::ChangedWhileProcessing))
    }
}

fn analysis_failed(e: AnalysisError) -> StepError {
    match e {
        AnalysisError::Cancelled => StepError::Cancelled,
        e => failed(e),
    }
}

fn tracking_failed(e: TrackingError) -> StepError {
    match e {
        TrackingError::Cancelled => StepError::Cancelled,
        e => failed(e),
    }
}

/// The folder of an item's analysis files under a session's records folder.
pub fn item_records_dir(records: &Path, item: ItemId) -> PathBuf {
    records.join(format!("item-{}", item.0))
}

/// An item's analysis files in a folder of its own. Until the analysis is complete the folder
/// goes with the item; after, it is kept for review or the second pass, and whoever keeps the
/// session removes it once nothing needs it.
pub struct ItemRecords {
    dir: PathBuf,
    complete: bool,
}

impl ItemRecords {
    fn create(records: &Path, item: ItemId) -> io::Result<Self> {
        std::fs::create_dir_all(records)?;
        let dir = item_records_dir(records, item);
        std::fs::create_dir(&dir)?;
        Ok(Self {
            dir,
            complete: false,
        })
    }

    fn existing(records: &Path, item: ItemId) -> Self {
        Self {
            dir: item_records_dir(records, item),
            complete: true,
        }
    }

    pub fn detections(&self) -> PathBuf {
        self.dir.join("detections")
    }

    fn backward(&self) -> PathBuf {
        self.dir.join("backward")
    }

    pub fn regions(&self) -> PathBuf {
        self.dir.join("regions")
    }
}

impl Drop for ItemRecords {
    fn drop(&mut self) {
        // Nothing can report a failure here. What is left is in the app's data folder, which the
        // session's own cleanup of its records folder reports.
        if !self.complete {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// What a video's analysis leaves for review and for a second pass resumed from it, besides its
/// evidence. The records themselves stay in the item's folder.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoAnalysis {
    /// [`Source::timeline_digest`].
    pub timeline: Digest,
    /// Source frames, the frames the records and the edits count.
    pub frames: u64,
    /// The display size the regions are in.
    pub size: (u32, u32),
    /// From the first frame shown to the end of the last, which with `frames` gives the average
    /// length of a frame.
    pub seconds: f64,
    pub tracks: Vec<TrackSummary>,
}

/// A video's analysis waiting for its second pass.
pub struct Analyzed {
    source: Source,
    records: ItemRecords,
}

impl Analyzed {
    pub fn records(&self) -> &ItemRecords {
        &self.records
    }
}

/// The rate the tracking windows are counted at: the source's own, frames per second on average,
/// or the output's when the source gives none.
fn source_rate(source: &Source) -> cloakframe_core::timeline::Rational {
    source.spec.average_rate.unwrap_or(source.spec.rate)
}

/// A source frame's share of the item's progress, which counts pixels as photos do.
fn frame_pixels(spec: &cloakframe_core::timeline::TimelineSpec) -> u64 {
    let (width, height) = spec.display_size();
    u64::from(width) * u64::from(height)
}

fn seconds(source: &Source) -> f64 {
    source.frames.end().as_f64() - source.spec.start.as_f64()
}

/// The verified output under its temporary name, the source it was made from and the item's
/// records, which go once it is published.
pub struct Rendered {
    output: TempOutput,
    source: SourceStamp,
    records: PathBuf,
}

pub struct VideoProcessor {
    pub(super) tools: Tools,
    pub(super) settings: VideoSettings,
    /// The session's folder for its items' records.
    pub(super) records: PathBuf,
    /// Held for a whole second pass, so videos render one at a time: the encoder already keeps
    /// every core busy, and two 1080p encodes at once finished only 2–8% sooner than one after
    /// the other (M4 stage 4) while holding twice the memory.
    encoders: Mutex<Encoders>,
}

impl VideoProcessor {
    /// The work [`ItemProcessor`]'s progress will count for `input`, from ffprobe's report
    /// without its frame list.
    pub fn estimated_work(&self, input: &Path, cancel: &CancelToken) -> Option<u64> {
        let spec = crate::media::probe::probe(&self.tools.ffprobe, input, cancel).ok()?;
        Some(2 * spec.estimated_frames * frame_pixels(&spec))
    }

    /// Each item's records go in a folder of its own under `records`, made when it is analyzed.
    pub fn new(tools: Tools, settings: VideoSettings, records: PathBuf) -> Self {
        let encoders = Mutex::new(Encoders::new(settings.hardware_encode));
        Self {
            tools,
            settings,
            records,
            encoders,
        }
    }
}

/// Why a second pass failed. Only the encoder's own failures are worth another pass in
/// software.
#[derive(Debug)]
enum PassError {
    Decode(DecodeError),
    Encode(EncodeError),
    Verify(VerifyError),
    Mask(ApplyError),
    Regions(RecordError),
    Output(io::Error),
}

impl PassError {
    fn encoders_fault(&self) -> bool {
        match self {
            Self::Encode(EncodeError::Cancelled | EncodeError::Output(_)) => false,
            Self::Encode(_) => true,
            Self::Verify(VerifyError::Cancelled) => false,
            Self::Verify(_) => true,
            Self::Decode(_) | Self::Mask(_) | Self::Regions(_) | Self::Output(_) => false,
        }
    }
}

impl fmt::Display for PassError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(e) => e.fmt(f),
            Self::Encode(e) => e.fmt(f),
            Self::Verify(e) => e.fmt(f),
            Self::Mask(e) => e.fmt(f),
            Self::Regions(e) => write!(f, "the tracked regions cannot be read: {e}"),
            Self::Output(e) => write!(f, "cannot create the output: {e}"),
        }
    }
}

impl std::error::Error for PassError {}

impl Classify for PassError {
    fn kind(&self) -> FailureKind {
        match self {
            Self::Decode(e) => e.kind(),
            Self::Encode(e) => e.kind(),
            Self::Verify(_) => FailureKind::Encode,
            Self::Mask(e) => e.kind(),
            Self::Regions(e) => e.kind(),
            Self::Output(_) => FailureKind::Output,
        }
    }
}

/// Runs the pass with the encoder `pick` gives. If a hardware encoder fails it, the pass is made
/// again with `software`; only when that one succeeds was the hardware at fault, and the run gives
/// up on hardware (plan 4.4, as a failing GPU detector moves to the CPU in M3). A failure the
/// software encode shares, such as audio FFmpeg cannot read, fails the item and leaves hardware
/// to the next one.
fn encode_with_fallback<T>(
    encoders: &mut Encoders,
    notes: &Notes,
    software: Encoder,
    pick: impl FnOnce(&Encoders) -> Result<Encoder, EncodeError>,
    mut pass: impl FnMut(Encoder) -> Result<T, PassError>,
) -> Result<T, PassError> {
    let encoder = pick(encoders).map_err(PassError::Encode)?;
    match pass(encoder) {
        Err(e) if encoder.hardware && e.encoders_fault() => {
            let retried = pass(software);
            if retried.is_ok() {
                encoders.note_hardware_failure();
                notes.add(Note::EncodedAgainInSoftware {
                    encoder: encoder.name,
                    reason: e.to_string(),
                });
            }
            retried
        }
        result => result,
    }
}

/// What a second pass or a preview masks: in every frame the frame's tracked regions on their
/// track's mosaic grid, except the tracks the reviewer excluded, the reviewer's own tracks on
/// theirs, and the fixed areas.
pub(super) struct Masks<'a> {
    fixed: Vec<Region>,
    tracked: Option<(&'a Record<TrackRegion>, BTreeMap<TrackId, MosaicGrid>)>,
    excluded: &'a [TrackId],
    manual: Vec<(&'a ManualTrack, MosaicGrid)>,
    size: (u32, u32),
}

impl<'a> Masks<'a> {
    fn none(size: (u32, u32)) -> Self {
        Self {
            fixed: Vec::new(),
            tracked: None,
            excluded: &[],
            manual: Vec::new(),
            size,
        }
    }

    /// `tracked` is the region record with its [`track_grids`].
    pub(super) fn new(
        fixed: Vec<Region>,
        tracked: Option<(&'a Record<TrackRegion>, BTreeMap<TrackId, MosaicGrid>)>,
        edits: &'a VideoEdits,
        style: &MaskStyle,
        size: (u32, u32),
    ) -> Self {
        let manual = edits
            .added_tracks
            .iter()
            .map(|track| (track, manual_grid(track, style, size)))
            .collect();
        Self {
            fixed,
            tracked,
            excluded: &edits.excluded_tracks,
            manual,
            size,
        }
    }

    pub(super) fn regions(&self) -> Option<&'a Record<TrackRegion>> {
        self.tracked.as_ref().map(|(record, _)| *record)
    }

    /// Frame `k`'s regions into `out`, from the region record's entries of that frame.
    pub(super) fn frame(&self, k: u64, entries: &[TrackRegion], out: &mut Vec<Region>) {
        out.clear();
        if let Some((_, grids)) = &self.tracked {
            out.extend(
                entries
                    .iter()
                    .filter(|r| !self.excluded.contains(&r.track))
                    .map(|r| Region {
                        roll: r.roll,
                        grid: grids.get(&r.track).copied(),
                        ..Region::new(r.bbox)
                    }),
            );
        }
        out.extend(self.manual.iter().filter_map(|(track, grid)| {
            track.box_at(k, self.size).map(|b| Region {
                grid: Some(*grid),
                ..Region::new(b.bbox)
            })
        }));
        out.extend_from_slice(&self.fixed);
    }
}

/// A manual track's grid, on the cell its largest box needs, like a tracked one's.
fn manual_grid(track: &ManualTrack, style: &MaskStyle, (width, height): (u32, u32)) -> MosaicGrid {
    let cell = (track.first..=track.last)
        .filter_map(|k| track.box_at(k, (width, height)))
        .map(|b| MosaicGrid::cell_for(&b.bbox, style, width, height))
        .max()
        .unwrap_or_default();
    MosaicGrid::new(cell)
}

/// Each track's mosaic grid, on the cell its largest box needs: a track's cells keep their size
/// and place in every frame (plan 4.4).
pub(super) fn track_grids(
    regions: &Record<TrackRegion>,
    style: &MaskStyle,
    (width, height): (u32, u32),
) -> Result<BTreeMap<TrackId, MosaicGrid>, RecordError> {
    let mut cells: BTreeMap<TrackId, u32> = BTreeMap::new();
    let mut frames = regions.ascending()?;
    let mut entries = Vec::new();
    while frames.next_into(&mut entries)?.is_some() {
        for r in &entries {
            let cell = MosaicGrid::cell_for(&r.bbox, style, width, height);
            let largest = cells.entry(r.track).or_default();
            *largest = (*largest).max(cell);
        }
    }
    Ok(cells
        .into_iter()
        .map(|(track, cell)| (track, MosaicGrid::new(cell)))
        .collect())
}

pub(super) fn mask_frame(
    frame: &mut Rgb8Image,
    regions: &[Region],
    style: &MaskStyle,
) -> Result<(), ApplyError> {
    if regions.is_empty() {
        return Ok(());
    }
    let buffer = RgbImage::from_raw(frame.width, frame.height, std::mem::take(&mut frame.data))
        .expect("the reader sizes the buffer to the frame");
    let mut image = DynamicImage::ImageRgb8(buffer);
    let result = mask::apply(&mut image, regions, style, None);
    frame.data = image.into_rgb8().into_raw();
    result
}

impl VideoProcessor {
    /// One second pass with `encoder` into a new temporary output, which is returned checked.
    fn pass(
        &self,
        ctx: &ItemContext<'_>,
        source: &Source,
        masks: &Masks<'_>,
        encoder: Encoder,
    ) -> Result<TempOutput, PassError> {
        let Source { spec, frames } = source;
        let output = TempOutput::create(ctx.destination).map_err(PassError::Output)?;
        let total = frames.output_frames(spec);
        let mut writer = FrameWriter::start(
            &self.tools.ffmpeg,
            encoder,
            self.settings.quality,
            spec,
            total,
            (!self.settings.remove_audio).then_some(ctx.input),
            Destination::Made(output.temp_path()),
            ctx.cancel,
        )
        .map_err(PassError::Encode)?;
        let mut reader = FrameReader::start(
            &self.tools.ffmpeg,
            ctx.input,
            spec,
            frames,
            frame_size(spec, None),
            ctx.cancel,
        )
        .map_err(PassError::Decode)?;
        let mut frame = Rgb8Image {
            width: 0,
            height: 0,
            data: Vec::new(),
        };
        let mut tracked = match masks.regions() {
            Some(record) => Some(record.ascending().map_err(PassError::Regions)?),
            None => None,
        };
        let (mut entries, mut regions) = (Vec::new(), Vec::new());
        let mut read = 0;
        let listed = frames.len() as u64;
        let pixels = frame_pixels(spec);
        for k in frames.shown(spec) {
            // Each source frame is masked once, however many output frames show it.
            while read <= k {
                if !reader.next_into(&mut frame).map_err(PassError::Decode)? {
                    return Err(PassError::Decode(DecodeError::FrameCount {
                        listed: frames.len(),
                        decoded: read,
                    }));
                }
                if let Some(record) = &mut tracked {
                    let at = record.next_into(&mut entries).map_err(PassError::Regions)?;
                    if at != Some(read as u64) {
                        return Err(PassError::Regions(RecordError::Damaged(
                            "the regions are not the source's frames",
                        )));
                    }
                }
                masks.frame(read as u64, &entries, &mut regions);
                mask_frame(&mut frame, &regions, &self.settings.style).map_err(PassError::Mask)?;
                read += 1;
                ctx.progress
                    .set((listed + read as u64) * pixels, 2 * listed * pixels);
            }
            writer.write(&frame).map_err(PassError::Encode)?;
        }
        // The reader checks the decoded count against the list once it reaches the end.
        while reader.next_into(&mut frame).map_err(PassError::Decode)? {}
        writer.finish().map_err(PassError::Encode)?;
        let mut expected = Expected::of(spec, self.settings.codec, total);
        if self.settings.remove_audio {
            expected.audio.clear();
        }
        verify::check(
            &self.tools.ffprobe,
            &self.tools.ffmpeg,
            output.temp_path(),
            &expected,
            ctx.cancel,
        )
        .map_err(PassError::Verify)?;
        Ok(output)
    }

    pub(super) fn regions(&self, (width, height): (u32, u32)) -> Result<Vec<Region>, VideoError> {
        self.settings
            .masks
            .iter()
            .enumerate()
            .map(|(i, &bbox)| {
                let overlaps = bbox.x() < width as f32
                    && bbox.y() < height as f32
                    && bbox.right() > 0.0
                    && bbox.bottom() > 0.0
                    && bbox.area() > 0.0;
                if overlaps {
                    Ok(Region::new(bbox))
                } else {
                    Err(VideoError::MaskOutside {
                        mask: i,
                        size: (width, height),
                    })
                }
            })
            .collect()
    }
}

/// Runs `work` while another thread reads the source for its digest. The digest is of use only
/// if `work` succeeds, so a failure stops the read.
fn beside_a_digest<T>(
    ctx: &ItemContext<'_>,
    work: impl FnOnce() -> Result<T, StepError>,
) -> Result<(T, Digest), StepError> {
    let reading = ctx.cancel.child();
    std::thread::scope(|scope| {
        let digest = std::thread::Builder::new()
            .name("cloakframe-digest".into())
            .spawn_scoped(scope, || file_digest(ctx.input, &reading))
            .map_err(|e| StepError::failed(FailureKind::Other, e))?;
        let result = work();
        if result.is_err() {
            reading.cancel();
        }
        let digest = digest
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        let value = result?;
        let digest = digest.map_err(|e| {
            failed_unless_cancelled(ctx, StepError::failed(FailureKind::Unreadable, e))
        })?;
        Ok((value, digest))
    })
}

impl VideoProcessor {
    fn first_pass(
        &self,
        ctx: &ItemContext<'_>,
        source: &Source,
        records: &ItemRecords,
    ) -> Result<(analysis::FirstPass, tracking::Tracked), StepError> {
        let settings = &self.settings.analysis;
        let listed = source.frames.len() as u64;
        let pixels = frame_pixels(&source.spec);
        let first = analysis::first_pass(
            &self.tools.ffmpeg,
            ctx.input,
            source,
            ctx.detector,
            settings,
            &records.detections(),
            ctx.cancel,
            |done| ctx.progress.set(done * pixels, 2 * listed * pixels),
        )
        .map_err(analysis_failed)?;
        let record = Record::<Detected>::open(&records.detections()).map_err(failed)?;
        let rate = source_rate(source);
        let kinds: Vec<KindTracking> = settings
            .kinds()
            .into_iter()
            .map(|kind| {
                KindTracking::new(kind, settings.track_score(kind), rate, &settings.windows)
            })
            .collect();
        let tracked = tracking::track(
            &record,
            &records.backward(),
            &records.regions(),
            &kinds,
            ctx.cancel,
        )
        .map_err(tracking_failed)?;
        Ok((first, tracked))
    }

    /// The source and records of an analysis stored for review, if the source still has the
    /// analyzed timeline.
    fn reopen(
        &self,
        ctx: &ItemContext<'_>,
        analysis: &Analysis,
        video: &VideoAnalysis,
    ) -> Result<Analyzed, StepError> {
        let stamp = analysis.source.expect("analysis records the source");
        if SourceStamp::of(ctx.input).ok() != Some(stamp) {
            return Err(failed(VideoError::ChangedSinceAnalysis));
        }
        let source = source::open(&self.tools.ffprobe, ctx.input, ctx.cancel)
            .map_err(|e| failed_unless_cancelled(ctx, e))?;
        unchanged(ctx, stamp)?;
        if source.timeline_digest() != video.timeline {
            return Err(failed(VideoError::ChangedSinceAnalysis));
        }
        Ok(Analyzed {
            source,
            records: ItemRecords::existing(&self.records, ctx.item),
        })
    }
}

impl ItemProcessor for VideoProcessor {
    type Prepared = Analyzed;
    type Rendered = Rendered;

    /// The first half of the item's progress is the first pass, a frame's pixels per source frame.
    fn analyze(&self, ctx: &ItemContext<'_>) -> Result<(Analysis, Analyzed), StepError> {
        let before = SourceStamp::of(ctx.input)
            .map_err(|e| StepError::failed(FailureKind::Unreadable, e))?;
        let source = source::open(&self.tools.ffprobe, ctx.input, ctx.cancel)
            .map_err(|e| failed_unless_cancelled(ctx, e))?;
        unchanged(ctx, before)?;
        self.regions(source.spec.display_size()).map_err(failed)?;
        let mut records = ItemRecords::create(&self.records, ctx.item)
            .map_err(|e| StepError::failed(FailureKind::Other, e))?;
        let ((first, tracked), input) =
            beside_a_digest(ctx, || self.first_pass(ctx, &source, &records))?;
        unchanged(ctx, before)?;
        if first.reran_on_cpu {
            ctx.notes.add(Note::AnalyzedAgainOnCpu);
        }
        if tracked.tracks.is_empty() {
            ctx.notes.add(Note::NothingDetected);
        } else {
            let count = |kind| tracked.tracks.iter().filter(|t| t.kind == kind).count() as u32;
            ctx.notes.add(Note::Tracked {
                faces: count(DetectionKind::Face),
                plates: count(DetectionKind::Plate),
            });
        }
        let mut evidence = first.capped;
        evidence.extend(tracked.gaps);
        let analysis = Analysis {
            evidence,
            source: Some(before),
            input: Some(input),
            video: Some(VideoAnalysis {
                timeline: source.timeline_digest(),
                frames: source.frames.len() as u64,
                size: source.spec.display_size(),
                seconds: seconds(&source),
                tracks: tracked.tracks,
            }),
            ..Analysis::default()
        };
        records.complete = true;
        Ok((analysis, Analyzed { source, records }))
    }

    fn render(
        &self,
        ctx: &ItemContext<'_>,
        request: &RenderRequest<'_>,
        prepared: Option<Analyzed>,
    ) -> Result<Rendered, StepError> {
        let analysis = request.analysis;
        let stamp = analysis.source.expect("analysis records the source");
        let video = analysis
            .video
            .as_ref()
            .ok_or_else(|| failed(VideoError::NotAnalyzed))?;
        let resumed = prepared.is_none();
        let analyzed = match prepared {
            Some(analyzed) => analyzed,
            None => self.reopen(ctx, analysis, video)?,
        };
        let source = &analyzed.source;
        let size = source.spec.display_size();
        let record;
        let masks = if request.basis == PublishBasis::ExportedWithoutAnonymization {
            Masks::none(size)
        } else {
            let fixed = self.regions(size).map_err(failed)?;
            let style = &self.settings.style;
            let tracked = if video.tracks.is_empty() {
                None
            } else {
                record =
                    Record::<TrackRegion>::open(&analyzed.records.regions()).map_err(failed)?;
                let grids = track_grids(&record, style, size).map_err(failed)?;
                Some((&record, grids))
            };
            Masks::new(fixed, tracked, &request.edits.video, style, size)
        };
        let mut encoders = self.encoders.lock().unwrap_or_else(|e| e.into_inner());
        unchanged(ctx, stamp)?;
        let mut second_pass = || {
            encode_with_fallback(
                &mut encoders,
                ctx.notes,
                Encoder::software(self.settings.codec),
                |encoders| {
                    encoders.pick(
                        &self.tools.ffmpeg,
                        self.settings.codec,
                        &source.spec,
                        ctx.cancel,
                    )
                },
                |encoder| self.pass(ctx, source, &masks, encoder),
            )
            .map_err(|e| failed_unless_cancelled(ctx, e))
        };
        // A pass straight after analysis renders the bytes the first pass read; a resumed one
        // reads them again for the digest the review was bound to.
        let output = if resumed {
            let (output, input) = beside_a_digest(ctx, second_pass)?;
            if analysis.input != Some(input) {
                return Err(failed(VideoError::ChangedSinceAnalysis));
            }
            output
        } else {
            second_pass()?
        };
        drop(encoders);
        unchanged(ctx, stamp)?;
        let (width, height) = encoded_size(size);
        if (width, height) != size {
            ctx.notes.add(Note::Padded { width, height });
        }
        Ok(Rendered {
            output,
            source: stamp,
            records: analyzed.records.dir.clone(),
        })
    }

    fn publish(
        &self,
        ctx: &ItemContext<'_>,
        rendered: Rendered,
        permit: &PublishPermit,
    ) -> Result<(), StepError> {
        unchanged(ctx, rendered.source)?;
        match rendered.output.publish(permit) {
            Ok(published) => {
                for w in &published.warnings {
                    ctx.notes.add(w.note());
                }
                match std::fs::remove_dir_all(&rendered.records) {
                    Err(e) if e.kind() != io::ErrorKind::NotFound => {
                        ctx.notes.add(Note::RecordsLeft);
                    }
                    _ => {}
                }
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                Err(failed(VideoError::NameTaken))
            }
            Err(e) => Err(StepError::failed(FailureKind::Output, e)),
        }
    }

    fn fingerprint(&self, _: &Path) -> Fingerprint {
        Fingerprint {
            models: self.settings.analysis.models.digest(),
            settings: self.settings.digest(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::{DetectorHandle, NoDetections};
    use crate::events::ProgressBoard;
    use crate::media::process::ToolError;
    use crate::test_support::{TestDir, fixtures, permit, required_env};
    use cloakframe_core::item::ItemId;
    use cloakframe_core::review::Edits;
    use std::time::{Duration, SystemTime};

    /// Runs `step` with a context for `input` published to `out/clip.mp4`.
    fn with_context<T>(input: &std::path::Path, step: impl FnOnce(&ItemContext<'_>) -> T) -> T {
        let destination = input.parent().unwrap().join("out/clip.mp4");
        let detector = DetectorHandle::spawn(Box::new(NoDetections)).unwrap();
        let notes = Notes::default();
        let ctx = ItemContext {
            item: ItemId(1),
            input,
            destination: &destination,
            cancel: &CancelToken::new(),
            detector: &detector,
            progress: ProgressBoard::default().reporter(ItemId(1)),
            notes: &notes,
        };
        step(&ctx)
    }

    fn touch(path: &std::path::Path) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(60))
            .unwrap();
    }

    fn changed(result: Result<impl Sized, StepError>) -> bool {
        matches!(result, Err(StepError::Failed { kind: FailureKind::SourceChanged, error }) if error.to_string() == VideoError::ChangedWhileProcessing.to_string())
    }

    /// Both passes count a frame's pixels, as photos do, so a run's items share a unit; the
    /// estimate from the report alone is the total the passes count.
    #[test]
    fn progress_counts_pixels_and_the_estimate_is_its_total() {
        let dir = TestDir::new();
        let input = dir.path().join("clip.mp4");
        std::fs::copy(
            fixtures().join("defects/P-5/odd-641x361-audio-longer.mp4"),
            &input,
        )
        .unwrap();
        let processor = VideoProcessor::new(
            Tools {
                ffmpeg: required_env("CLOAKFRAME_FFMPEG"),
                ffprobe: required_env("CLOAKFRAME_FFPROBE"),
            },
            VideoSettings {
                hardware_encode: false,
                ..VideoSettings::default()
            },
            dir.path().join("records"),
        );
        let estimate = processor.estimated_work(&input, &CancelToken::new());
        let destination = dir.path().join("out/clip.mp4");
        let detector = DetectorHandle::spawn(Box::new(NoDetections)).unwrap();
        let (notes, board, cancel) = (
            Notes::default(),
            ProgressBoard::default(),
            CancelToken::new(),
        );
        let ctx = ItemContext {
            item: ItemId(1),
            input: &input,
            destination: &destination,
            cancel: &cancel,
            detector: &detector,
            progress: board.reporter(ItemId(1)),
            notes: &notes,
        };
        let (analysis, analyzed) = processor.analyze(&ctx).unwrap();
        let video = analysis.video.as_ref().unwrap();
        let total = 2 * video.frames * u64::from(video.size.0) * u64::from(video.size.1);
        let progress = board.snapshot()[&ItemId(1)];
        assert_eq!((progress.done, progress.total), (total / 2, total));
        assert_eq!(estimate, Some(total));
        assert!(
            (video.seconds - video.frames as f64 / 30.0).abs() < 1e-6,
            "{}",
            video.seconds
        );
        processor
            .render(
                &ctx,
                &RenderRequest {
                    basis: PublishBasis::AutomaticUnreviewed,
                    analysis: &analysis,
                    edits: &Edits::default(),
                },
                Some(analyzed),
            )
            .unwrap();
        let progress = board.snapshot()[&ItemId(1)];
        assert_eq!((progress.done, progress.total), (total, total));
    }

    /// Regions that end before the source's frames fail the pass instead of leaving the rest of
    /// the frames unmasked.
    #[test]
    fn regions_short_of_the_sources_frames_fail_the_item() {
        use crate::video::record::{Order, RecordWriter};
        use cloakframe_core::detection::DetectionKind;
        let dir = TestDir::new();
        let input = dir.path().join("clip.mp4");
        std::fs::copy(
            fixtures().join("defects/P-5/odd-641x361-audio-longer.mp4"),
            &input,
        )
        .unwrap();
        let tools = Tools {
            ffmpeg: required_env("CLOAKFRAME_FFMPEG"),
            ffprobe: required_env("CLOAKFRAME_FFPROBE"),
        };
        let source = source::open(&tools.ffprobe, &input, &CancelToken::new()).unwrap();
        let processor = VideoProcessor::new(
            tools,
            VideoSettings {
                hardware_encode: false,
                ..VideoSettings::default()
            },
            dir.path().join("records"),
        );
        let records = ItemRecords::create(&dir.path().join("records"), ItemId(1)).unwrap();
        let mut writer = RecordWriter::create(&records.regions(), Order::Ascending).unwrap();
        for k in 0..source.frames.len() as u64 - 1 {
            let region = TrackRegion {
                kind: DetectionKind::Face,
                track: 1,
                bbox: Rect::new(10.0, 10.0, 40.0, 40.0).unwrap(),
                score: 0.9,
                roll: None,
                interpolated: false,
            };
            writer.frame(k, &[region]).unwrap();
        }
        writer.finish(&[]).unwrap();
        let track = TrackSummary {
            id: 1,
            kind: DetectionKind::Face,
            first: 0,
            last: source.frames.len() as u64 - 2,
            detections: 1,
            strong: 1,
            low_confidence: false,
        };
        let video = VideoAnalysis {
            timeline: source.timeline_digest(),
            frames: source.frames.len() as u64,
            size: source.spec.display_size(),
            seconds: seconds(&source),
            tracks: vec![track],
        };
        let analyzed = Analyzed { source, records };
        let analysis = Analysis {
            source: Some(SourceStamp::of(&input).unwrap()),
            video: Some(video),
            ..Analysis::default()
        };
        with_context(&input, |ctx| {
            let result = processor.render(
                ctx,
                &RenderRequest {
                    basis: PublishBasis::AutomaticUnreviewed,
                    analysis: &analysis,
                    edits: &Edits::default(),
                },
                Some(analyzed),
            );
            let Err(StepError::Failed { error: e, .. }) = result else {
                panic!("rendered with regions short of the frames");
            };
            assert!(
                e.to_string().contains("the tracked regions cannot be read"),
                "{e}"
            );
        });
        let out = dir.path().join("out");
        assert!(!out.exists() || std::fs::read_dir(&out).unwrap().next().is_none());
    }

    /// The source is read again by each pass; one changed in between is not rendered or
    /// published, and nothing is left in the output folder.
    #[test]
    fn a_source_that_changes_between_the_passes_or_before_publishing_fails_the_item() {
        let dir = TestDir::new();
        let input = dir.path().join("clip.mp4");
        std::fs::copy(
            fixtures().join("defects/P-5/odd-641x361-audio-longer.mp4"),
            &input,
        )
        .unwrap();
        let processor = VideoProcessor::new(
            Tools {
                ffmpeg: required_env("CLOAKFRAME_FFMPEG"),
                ffprobe: required_env("CLOAKFRAME_FFPROBE"),
            },
            VideoSettings {
                hardware_encode: false,
                masks: vec![Rect::new(0.0, 0.0, 8.0, 8.0).unwrap()],
                ..VideoSettings::default()
            },
            dir.path().join("records"),
        );
        let edits = Edits::default();
        let out = dir.path().join("out");

        with_context(&input, |ctx| {
            let (analysis, source) = processor.analyze(ctx).unwrap();
            touch(&input);
            assert!(changed(processor.render(
                ctx,
                &RenderRequest {
                    basis: PublishBasis::AutomaticUnreviewed,
                    analysis: &analysis,
                    edits: &edits
                },
                Some(source)
            )));
        });
        assert!(!out.exists() || std::fs::read_dir(&out).unwrap().next().is_none());
        // A complete analysis outlives a failed render; the session's cleanup removes it.
        let records = dir.path().join("records");
        assert!(records.join("item-1/regions").exists());
        std::fs::remove_dir_all(&records).unwrap();

        with_context(&input, |ctx| {
            let (analysis, source) = processor.analyze(ctx).unwrap();
            let rendered = processor
                .render(
                    ctx,
                    &RenderRequest {
                        basis: PublishBasis::AutomaticUnreviewed,
                        analysis: &analysis,
                        edits: &edits,
                    },
                    Some(source),
                )
                .unwrap();
            touch(&input);
            assert!(changed(processor.publish(ctx, rendered, &permit())));
        });
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0);
    }

    fn hardware() -> Encoder {
        Encoder {
            name: "h264_videotoolbox",
            hardware: true,
        }
    }

    fn software() -> Encoder {
        Encoder::software(OutputCodec::H264)
    }

    fn broken_encoder() -> PassError {
        PassError::Encode(EncodeError::Tool(ToolError::Io(
            io::ErrorKind::BrokenPipe.into(),
        )))
    }

    /// The encoders the pass was made with, and what came of it.
    fn run(
        hardware_first: bool,
        outcomes: Vec<Result<(), PassError>>,
    ) -> (Vec<Encoder>, Result<(), PassError>, Encoders, Notes) {
        let mut encoders = Encoders::new(true);
        let notes = Notes::default();
        let mut outcomes = outcomes.into_iter();
        let mut used = Vec::new();
        let result = encode_with_fallback(
            &mut encoders,
            &notes,
            software(),
            |e| {
                Ok(if hardware_first && !e.hardware_failed() {
                    hardware()
                } else {
                    software()
                })
            },
            |encoder| {
                used.push(encoder);
                outcomes.next().expect("a pass too many")
            },
        );
        (used, result, encoders, notes)
    }

    #[test]
    fn a_failing_hardware_encoder_is_replaced_by_software_for_the_item_and_the_run() {
        let failures = [
            broken_encoder(),
            PassError::Verify(VerifyError::FrameCount {
                expected: 60,
                decoded: 59,
            }),
        ];
        for failure in failures {
            let reason = failure.to_string();
            let (used, result, encoders, notes) = run(true, vec![Err(failure), Ok(())]);
            assert_eq!(used, [hardware(), software()]);
            result.unwrap();
            assert!(encoders.hardware_failed());
            assert_eq!(
                notes.into_vec(),
                [Note::EncodedAgainInSoftware {
                    encoder: hardware().name,
                    reason
                }]
            );
        }
    }

    #[test]
    fn failures_that_are_not_the_encoders_are_not_tried_again() {
        let failures = [
            PassError::Decode(DecodeError::FrameCount {
                listed: 60,
                decoded: 59,
            }),
            PassError::Encode(EncodeError::Cancelled),
            PassError::Verify(VerifyError::Cancelled),
            PassError::Output(io::ErrorKind::PermissionDenied.into()),
        ];
        for failure in failures {
            let (used, result, encoders, notes) = run(true, vec![Err(failure)]);
            assert_eq!(used, [hardware()]);
            assert!(result.is_err());
            assert!(!encoders.hardware_failed());
            assert!(notes.into_vec().is_empty());
        }
    }

    #[test]
    fn a_failing_software_encode_fails_the_item() {
        let (used, result, encoders, _) = run(false, vec![Err(broken_encoder())]);
        assert_eq!(used, [software()]);
        assert!(result.is_err());
        assert!(!encoders.hardware_failed());
    }

    /// Review 11: a failure software shares, such as audio FFmpeg cannot copy, is the item's
    /// and not the hardware's, which the next item still uses.
    #[test]
    fn a_software_retry_that_fails_too_fails_the_item_and_keeps_the_hardware() {
        let audio = PassError::Verify(VerifyError::Streams("no audio stream 1".to_owned()));
        let (used, result, encoders, notes) = run(true, vec![Err(broken_encoder()), Err(audio)]);
        assert_eq!(used, [hardware(), software()]);
        assert!(matches!(
            result,
            Err(PassError::Verify(VerifyError::Streams(_)))
        ));
        assert!(!encoders.hardware_failed());
        assert!(notes.into_vec().is_empty());
    }

    /// A track's grid is the cell its largest box needs, so none of its frames gets more cells
    /// than the strength; each track has its own.
    #[test]
    fn each_track_takes_the_cell_of_its_largest_box() {
        use crate::video::record::{Order, RecordWriter};
        use cloakframe_core::detection::DetectionKind;
        let dir = crate::test_support::TestDir::new();
        let path = dir.path().join("regions");
        let region = |track, side: f32| TrackRegion {
            kind: DetectionKind::Face,
            track,
            bbox: Rect::new(10.0, 10.0, side, side).unwrap(),
            score: 0.9,
            roll: None,
            interpolated: false,
        };
        let mut writer = RecordWriter::create(&path, Order::Ascending).unwrap();
        writer
            .frame(0, &[region(1, 40.0), region(2, 20.0)])
            .unwrap();
        writer.frame(1, &[region(1, 100.0)]).unwrap();
        writer
            .frame(2, &[region(1, 60.0), region(2, 30.0)])
            .unwrap();
        writer.finish(&[]).unwrap();
        let style = MaskStyle::default();
        let grids = track_grids(&Record::open(&path).unwrap(), &style, (640, 360)).unwrap();
        let cell = |side| MosaicGrid::cell_for(&region(1, side).bbox, &style, 640, 360);
        assert_eq!(
            grids,
            BTreeMap::from([
                (1, MosaicGrid::new(cell(100.0))),
                (2, MosaicGrid::new(cell(30.0)))
            ])
        );
        assert!(cell(100.0) > cell(60.0) && cell(30.0) > cell(20.0));
    }

    #[test]
    fn masks_must_overlap_the_frame() {
        let processor = |masks: Vec<Rect>| {
            VideoProcessor::new(
                Tools {
                    ffmpeg: PathBuf::new(),
                    ffprobe: PathBuf::new(),
                },
                VideoSettings {
                    masks,
                    ..VideoSettings::default()
                },
                PathBuf::new(),
            )
        };
        let rect = |x, y, w, h| Rect::new(x, y, w, h).unwrap();
        let inside = processor(vec![
            rect(0.0, 0.0, 10.0, 10.0),
            rect(630.0, 350.0, 50.0, 50.0),
        ]);
        assert_eq!(inside.regions((641, 361)).unwrap().len(), 2);
        for outside in [
            rect(641.0, 0.0, 10.0, 10.0),
            rect(0.0, 361.0, 10.0, 10.0),
            rect(-10.0, 0.0, 10.0, 10.0),
            rect(10.0, 10.0, 0.0, 10.0),
        ] {
            let p = processor(vec![rect(0.0, 0.0, 1.0, 1.0), outside]);
            assert!(matches!(
                p.regions((641, 361)),
                Err(VideoError::MaskOutside { mask: 1, .. })
            ));
        }
    }
}
