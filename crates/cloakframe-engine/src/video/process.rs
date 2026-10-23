//! The video pipeline as the runner's item processor (plan 4.4). Analysis reads the timeline,
//! detects in every frame and tracks into the item's records; the second pass decodes every
//! source frame at full size, masks the frame's tracked regions and the fixed areas the user
//! gives, encodes, and checks the output (P-4) before it waits for its publish permit. The source
//! is never copied; its identity is compared before the first pass, after it, between the passes,
//! after the second and right before publishing (plan 0.1).

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use cloakframe_core::detection::DetectionKind;
use cloakframe_core::geometry::Rect;
use cloakframe_core::item::{ItemId, PublishBasis, PublishPermit};
use cloakframe_core::mask::{MaskStyle, MosaicGrid, Region};
use cloakframe_core::review::Digest;
use cloakframe_core::tracking::TrackId;
use image::{DynamicImage, RgbImage};
use sha2::{Digest as _, Sha256};

use super::analysis::{self, AnalysisError, AnalysisSettings};
use super::record::{Detected, Record, RecordError, TrackRegion};
use super::tracking::{self, KindTracking, TrackSummary, TrackingError};
use crate::detect::Rgb8Image;
use crate::identity::SourceStamp;
use crate::media::decode::{DecodeError, FrameReader, frame_size};
use crate::media::encode::{
    Destination, EncodeError, Encoder, Encoders, FrameWriter, OutputCodec, Quality, encoded_size,
};
use crate::media::source::{self, Source};
use crate::media::verify::{self, Expected, VerifyError};
use crate::photo::mask::{self, ApplyError};
use crate::publish::TempOutput;
use crate::run::{
    Analysis, Fingerprint, ItemContext, ItemProcessor, Note, Notes, RenderRequest, StepError,
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
            "codec {:?} quality {:?} hardware {}\nstyle {:?} {:?} padding {:08x} soft {} mosaic {}\n",
            self.codec,
            self.quality,
            self.hardware_encode,
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
    /// Reviews of videos come with their detections (M5); until then a video is not resumed.
    NotResumable,
    /// The source changed between reading its timeline and publishing the output.
    ChangedWhileProcessing,
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
            Self::NotResumable => f.write_str("videos cannot be resumed from a review yet"),
            Self::ChangedWhileProcessing => {
                f.write_str("the source changed while it was processed")
            }
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

fn failed(e: impl std::error::Error + Send + Sync + 'static) -> StepError {
    StepError::Failed(Box::new(e))
}

/// A tool stopped by the item's cancel reports its own error; the cancel is what happened.
fn failed_unless_cancelled(
    ctx: &ItemContext<'_>,
    e: impl std::error::Error + Send + Sync + 'static,
) -> StepError {
    if ctx.cancel.is_cancelled() {
        StepError::Cancelled
    } else {
        failed(e)
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

/// An item's analysis files in a folder of its own, removed with the item.
pub struct ItemRecords {
    dir: PathBuf,
}

impl ItemRecords {
    fn create(records: &Path, item: ItemId) -> io::Result<Self> {
        std::fs::create_dir_all(records)?;
        let dir = records.join(format!("item-{}", item.0));
        std::fs::create_dir(&dir)?;
        Ok(Self { dir })
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
        // run's own cleanup of its records folder reports.
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A video's analysis waiting for its second pass.
pub struct Analyzed {
    source: Source,
    records: ItemRecords,
    tracks: Vec<TrackSummary>,
}

impl Analyzed {
    pub fn records(&self) -> &ItemRecords {
        &self.records
    }

    pub fn tracks(&self) -> &[TrackSummary] {
        &self.tracks
    }
}

/// The rate the tracking windows are counted at: the source's own, frames per second on average,
/// or the output's when the source gives none.
fn source_rate(source: &Source) -> cloakframe_core::timeline::Rational {
    source.spec.average_rate.unwrap_or(source.spec.rate)
}

/// The verified output under its temporary name and the source it was made from.
pub struct Rendered {
    output: TempOutput,
    source: SourceStamp,
}

pub struct VideoProcessor {
    tools: Tools,
    settings: VideoSettings,
    /// The run's folder for its items' records.
    records: PathBuf,
    /// Held for a whole second pass, so videos render one at a time: the encoder already keeps
    /// every core busy, and two 1080p encodes at once finished only 2–8% sooner than one after
    /// the other (M4 stage 4) while holding twice the memory.
    encoders: Mutex<Encoders>,
}

impl VideoProcessor {
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

/// Runs the pass with the encoder `pick` gives. If a hardware encoder fails it, the run gives
/// up on hardware and the pass is made again with whatever `pick` gives then, which is software
/// (plan 4.4, as a failing GPU detector moves to the CPU in M3).
fn encode_with_fallback<T>(
    encoders: &mut Encoders,
    notes: &Notes,
    mut pick: impl FnMut(&Encoders) -> Result<Encoder, EncodeError>,
    mut pass: impl FnMut(Encoder) -> Result<T, PassError>,
) -> Result<T, PassError> {
    let encoder = pick(encoders).map_err(PassError::Encode)?;
    match pass(encoder) {
        Err(e) if encoder.hardware && e.encoders_fault() => {
            encoders.note_hardware_failure();
            notes.add(Note::EncodedAgainInSoftware);
            pass(pick(encoders).map_err(PassError::Encode)?)
        }
        result => result,
    }
}

/// What a second pass masks: in every frame the fixed areas and, from the region record, the
/// frame's tracked regions on their track's mosaic grid.
struct Masks<'a> {
    fixed: Vec<Region>,
    tracked: Option<(&'a Record<TrackRegion>, BTreeMap<TrackId, MosaicGrid>)>,
}

impl Masks<'_> {
    fn none() -> Self {
        Self {
            fixed: Vec::new(),
            tracked: None,
        }
    }
}

/// Each track's mosaic grid, on the cell its largest box needs: a track's cells keep their size
/// and place in every frame (plan 4.4).
fn track_grids(
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

fn mask_frame(
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
            Some(ctx.input),
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
        let mut tracked = match &masks.tracked {
            Some((record, grids)) => Some((record.ascending().map_err(PassError::Regions)?, grids)),
            None => None,
        };
        let (mut entries, mut regions) = (Vec::new(), Vec::new());
        let mut read = 0;
        let listed = frames.len() as u64;
        for k in frames.shown(spec) {
            // Each source frame is masked once, however many output frames show it.
            while read <= k {
                if !reader.next_into(&mut frame).map_err(PassError::Decode)? {
                    return Err(PassError::Decode(DecodeError::FrameCount {
                        listed: frames.len(),
                        decoded: read,
                    }));
                }
                regions.clear();
                if let Some((record, grids)) = &mut tracked {
                    let at = record.next_into(&mut entries).map_err(PassError::Regions)?;
                    if at != Some(read as u64) {
                        return Err(PassError::Regions(RecordError::Damaged(
                            "the regions are not the source's frames",
                        )));
                    }
                    regions.extend(entries.iter().map(|r| Region {
                        roll: r.roll,
                        grid: grids.get(&r.track).copied(),
                        ..Region::new(r.bbox)
                    }));
                }
                regions.extend_from_slice(&masks.fixed);
                mask_frame(&mut frame, &regions, &self.settings.style).map_err(PassError::Mask)?;
                read += 1;
                ctx.progress.set(listed + read as u64, 2 * listed);
            }
            writer.write(&frame).map_err(PassError::Encode)?;
        }
        // The reader checks the decoded count against the list once it reaches the end.
        while reader.next_into(&mut frame).map_err(PassError::Decode)? {}
        writer.finish().map_err(PassError::Encode)?;
        let expected = Expected::of(spec, self.settings.codec, total);
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

    fn regions(&self, (width, height): (u32, u32)) -> Result<Vec<Region>, VideoError> {
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

impl ItemProcessor for VideoProcessor {
    type Prepared = Analyzed;
    type Rendered = Rendered;

    /// The first half of the item's progress is the first pass, a unit per source frame.
    fn analyze(&self, ctx: &ItemContext<'_>) -> Result<(Analysis, Analyzed), StepError> {
        let before = SourceStamp::of(ctx.input).map_err(failed)?;
        let source = source::open(&self.tools.ffprobe, ctx.input, ctx.cancel)
            .map_err(|e| failed_unless_cancelled(ctx, e))?;
        unchanged(ctx, before)?;
        self.regions(source.spec.display_size()).map_err(failed)?;
        let records = ItemRecords::create(&self.records, ctx.item).map_err(failed)?;
        let settings = &self.settings.analysis;
        let listed = source.frames.len() as u64;
        let first = analysis::first_pass(
            &self.tools.ffmpeg,
            ctx.input,
            &source,
            ctx.detector,
            settings,
            &records.detections(),
            ctx.cancel,
            |done| ctx.progress.set(done, 2 * listed),
        )
        .map_err(analysis_failed)?;
        unchanged(ctx, before)?;
        let record = Record::<Detected>::open(&records.detections()).map_err(failed)?;
        let rate = source_rate(&source);
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
            ..Analysis::default()
        };
        Ok((
            analysis,
            Analyzed {
                source,
                records,
                tracks: tracked.tracks,
            },
        ))
    }

    fn render(
        &self,
        ctx: &ItemContext<'_>,
        request: &RenderRequest<'_>,
        prepared: Option<Analyzed>,
    ) -> Result<Rendered, StepError> {
        let analyzed = prepared.ok_or_else(|| failed(VideoError::NotResumable))?;
        let source = &analyzed.source;
        let stamp = request
            .analysis
            .source
            .expect("analysis records the source");
        let size = source.spec.display_size();
        let record;
        let masks = if request.basis == PublishBasis::ExportedWithoutAnonymization {
            Masks::none()
        } else {
            let fixed = self.regions(size).map_err(failed)?;
            let tracked = if analyzed.tracks.is_empty() {
                None
            } else {
                record =
                    Record::<TrackRegion>::open(&analyzed.records.regions()).map_err(failed)?;
                let grids = track_grids(&record, &self.settings.style, size).map_err(failed)?;
                Some((&record, grids))
            };
            Masks { fixed, tracked }
        };
        let mut encoders = self.encoders.lock().unwrap_or_else(|e| e.into_inner());
        unchanged(ctx, stamp)?;
        let output = encode_with_fallback(
            &mut encoders,
            ctx.notes,
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
        .map_err(|e| failed_unless_cancelled(ctx, e))?;
        drop(encoders);
        unchanged(ctx, stamp)?;
        let (width, height) = encoded_size(size);
        if (width, height) != size {
            ctx.notes.add(Note::Padded { width, height });
        }
        Ok(Rendered {
            output,
            source: stamp,
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
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                Err(failed(VideoError::NameTaken))
            }
            Err(e) => Err(failed(e)),
        }
    }

    fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            models: self.settings.analysis.models.digest(),
            settings: self.settings.digest(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel::CancelToken;
    use crate::detect::{DetectorHandle, NoDetections};
    use crate::events::ProgressBoard;
    use crate::media::process::ToolError;
    use crate::test_support::{TestDir, fixtures, permit, required_env};
    use cloakframe_core::item::ItemId;
    use cloakframe_core::review::PhotoEdits;
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
        matches!(result, Err(StepError::Failed(e)) if e.to_string() == VideoError::ChangedWhileProcessing.to_string())
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
        let analyzed = Analyzed {
            source,
            records,
            tracks: vec![track],
        };
        let analysis = Analysis {
            source: Some(SourceStamp::of(&input).unwrap()),
            ..Analysis::default()
        };
        with_context(&input, |ctx| {
            let result = processor.render(
                ctx,
                &RenderRequest {
                    basis: PublishBasis::AutomaticUnreviewed,
                    analysis: &analysis,
                    edits: &PhotoEdits::default(),
                },
                Some(analyzed),
            );
            let Err(StepError::Failed(e)) = result else {
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
        let edits = PhotoEdits::default();
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
            let (used, result, encoders, notes) = run(true, vec![Err(failure), Ok(())]);
            assert_eq!(used, [hardware(), software()]);
            result.unwrap();
            assert!(encoders.hardware_failed());
            assert_eq!(notes.into_vec(), [Note::EncodedAgainInSoftware]);
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

    #[test]
    fn a_software_retry_that_fails_too_fails_the_item() {
        let (used, result, _, notes) =
            run(true, vec![Err(broken_encoder()), Err(broken_encoder())]);
        assert_eq!(used, [hardware(), software()]);
        assert!(result.is_err());
        assert_eq!(notes.into_vec(), [Note::EncodedAgainInSoftware]);
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
