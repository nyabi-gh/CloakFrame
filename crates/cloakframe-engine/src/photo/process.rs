//! The photo pipeline as the runner's item processor (plan 4.3): read the source once, decode,
//! apply the color policy, detect on the sRGB copy, mask, encode, and publish only while the
//! source is still the file that was read.

use std::fmt;
use std::io::{self, Write};
use std::path::Path;

use cloakframe_core::coverage::{CoverageEvidence, Problem};
use cloakframe_core::detection::{DetectionKind, Thresholds};
use cloakframe_core::item::{PublishBasis, PublishPermit};
use cloakframe_core::mask::{MaskError, MaskStyle, Method, Overlay};
use cloakframe_core::review::Digest;
use sha2::{Digest as _, Sha256};

use super::budget::{BudgetError, MemoryBudget};
use super::color::{self, ColorOutcome};
use super::decode::{self, Decoded, ReadError, Source};
use super::encode;
use super::format::{ImageFormat, sniff};
use super::mask;
use super::metadata::MetadataPolicy;
use crate::detect::models::DetectionModels;
use crate::detect::{Found, Rgb8Image, tiles};
use crate::identity::{SourceStamp, content_digest};
use crate::publish::TempOutput;
use crate::run::{
    Analysis, Fingerprint, ItemContext, ItemProcessor, Note, RenderRequest, StepError,
};

/// Everything besides the input that decides a photo's output. A stored review is bound to all of
/// it (plan 4.2).
#[derive(Debug, Clone, PartialEq)]
pub struct PhotoSettings {
    /// The detector the run is given must load these models.
    pub models: DetectionModels,
    pub thresholds: Thresholds,
    /// Detect in overlapping tiles as well as the whole view, for faces too small for it.
    pub tiles: bool,
    pub style: MaskStyle,
    /// The user's image for [`Method::CustomImage`].
    pub overlay: Option<Overlay>,
    pub metadata: MetadataPolicy,
}

impl Default for PhotoSettings {
    fn default() -> Self {
        let models = DetectionModels::default();
        Self {
            models,
            thresholds: models.default_thresholds(),
            tiles: true,
            style: MaskStyle::default(),
            overlay: None,
            metadata: MetadataPolicy::default(),
        }
    }
}

impl PhotoSettings {
    /// The models are the fingerprint's other half; the thresholds of a kind the models do not
    /// detect change nothing.
    fn digest(&self) -> Digest {
        let mut h = Sha256::new();
        for kind in self.models.ids().map(|id| id.kind()) {
            let t = self.thresholds.of(kind);
            let name = match kind {
                DetectionKind::Face => "face",
                DetectionKind::Plate => "plate",
            };
            h.update(format!(
                "{name} score {:08x} nms {:08x}\n",
                t.score().to_bits(),
                t.nms_iou().to_bits()
            ));
        }
        h.update(format!("tiles {}\n", self.tiles));
        let s = &self.style;
        h.update(format!(
            "style {:?} {:?} padding {:08x} soft {} mosaic {}\nmetadata {:?}\n",
            s.method,
            s.shape,
            s.padding.ratio().to_bits(),
            s.soft_edges,
            s.mosaic_strength.cells(),
            self.metadata
        ));
        if let Some(overlay) = &self.overlay {
            h.update(format!(
                "overlay {}x{}\n",
                overlay.width(),
                overlay.height()
            ));
            h.update(overlay.rgba());
        }
        Digest(h.finalize().into())
    }
}

#[derive(Debug)]
pub enum PhotoError {
    /// A resumed review's source is not the file the review was made on.
    ChangedSinceAnalysis,
    /// The source changed between reading it and publishing the output.
    ChangedWhileProcessing,
    /// Something appeared under the output name during the run; it is not replaced.
    NameTaken,
    /// The encoded output does not read back as the expected format and size.
    UnreadableOutput(String),
}

impl fmt::Display for PhotoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChangedSinceAnalysis => {
                f.write_str("the source changed since it was analyzed; run it again")
            }
            Self::ChangedWhileProcessing => {
                f.write_str("the source changed while it was processed")
            }
            Self::NameTaken => f.write_str("the output name was taken during the run"),
            Self::UnreadableOutput(why) => write!(f, "the encoded output is not readable: {why}"),
        }
    }
}

impl std::error::Error for PhotoError {}

fn failed(e: impl std::error::Error + Send + Sync + 'static) -> StepError {
    StepError::Failed(Box::new(e))
}

fn read_failed(e: ReadError) -> StepError {
    match e {
        ReadError::Memory(BudgetError::Cancelled) => StepError::Cancelled,
        e => failed(e),
    }
}

fn checkpoint(ctx: &ItemContext<'_>) -> Result<(), StepError> {
    if ctx.cancel.is_cancelled() {
        Err(StepError::Cancelled)
    } else {
        Ok(())
    }
}

/// The source as it was read, decoded and color-managed. Holds the item's memory permit.
pub struct Prepared {
    source: Source,
    decoded: Decoded,
    color: ColorOutcome,
}

impl Prepared {
    fn pixels(&self) -> u64 {
        u64::from(self.decoded.image.width()) * u64::from(self.decoded.image.height())
    }
}

/// The verified output under its temporary name and the source it was made from.
pub struct Rendered {
    output: TempOutput,
    source: SourceStamp,
}

pub struct PhotoProcessor {
    settings: PhotoSettings,
    budget: MemoryBudget,
}

impl PhotoProcessor {
    pub fn new(settings: PhotoSettings) -> Result<Self, MaskError> {
        if settings.style.method == Method::CustomImage && settings.overlay.is_none() {
            return Err(MaskError::MissingOverlay);
        }
        Ok(Self {
            settings,
            budget: MemoryBudget::for_this_computer(),
        })
    }

    /// Shares `budget` instead of a quarter of this computer's memory.
    #[must_use]
    pub fn with_budget(mut self, budget: MemoryBudget) -> Self {
        self.budget = budget;
        self
    }

    fn prepare(&self, ctx: &ItemContext<'_>) -> Result<Prepared, StepError> {
        let source = decode::read_source(ctx.input).map_err(read_failed)?;
        checkpoint(ctx)?;
        let mut decoded = decode::decode(&source, &self.budget, ctx.cancel).map_err(read_failed)?;
        let color = color::apply(&source, &mut decoded.image).map_err(failed)?;
        Ok(Prepared {
            source,
            decoded,
            color,
        })
    }
}

/// The destination's extension decides, as in 1.12.1; a name without a known image extension
/// keeps the source's format.
fn output_format(destination: &Path, source: ImageFormat) -> ImageFormat {
    destination
        .extension()
        .and_then(|e| e.to_str())
        .and_then(ImageFormat::from_extension)
        .unwrap_or(source)
}

fn verify(bytes: &[u8], format: ImageFormat, size: (u32, u32)) -> Result<(), PhotoError> {
    let unreadable = |why: String| PhotoError::UnreadableOutput(why);
    if sniff(bytes) != Some(format) {
        return Err(unreadable(format!("not a {format} file")));
    }
    let read = decode::header_dimensions(bytes, format).map_err(|e| unreadable(e.to_string()))?;
    if read != size {
        return Err(unreadable(format!(
            "{}×{} instead of {}×{}",
            read.0, read.1, size.0, size.1
        )));
    }
    Ok(())
}

impl PhotoProcessor {
    /// The whole view, and in tile mode the tiles, each a request to the inference thread so the
    /// run can be cancelled between them. Analysis is the first half of the item's progress.
    fn detect(
        &self,
        ctx: &ItemContext<'_>,
        copy: Rgb8Image,
        pixels: u64,
    ) -> Result<Found, StepError> {
        let thresholds = self.settings.thresholds;
        let layout = if self.settings.tiles {
            tiles::layout(copy.width, copy.height)
        } else {
            Vec::new()
        };
        let views = layout.len() as u64 + 1;
        let mut tiled = Vec::with_capacity(layout.len());
        for (done, tile) in layout.into_iter().enumerate() {
            checkpoint(ctx)?;
            let found = ctx
                .detector
                .detect(tile.image(&copy), thresholds)
                .map_err(failed)?;
            tiled.push((tile, found));
            ctx.progress
                .set(pixels * (done as u64 + 1) / views, 2 * pixels);
        }
        checkpoint(ctx)?;
        let whole = ctx.detector.detect(copy, thresholds).map_err(failed)?;
        Ok(tiles::merge(whole, tiled, &thresholds))
    }
}

impl ItemProcessor for PhotoProcessor {
    type Prepared = Prepared;
    type Rendered = Rendered;

    fn analyze(&self, ctx: &ItemContext<'_>) -> Result<(Analysis, Prepared), StepError> {
        let prepared = self.prepare(ctx)?;
        let copy =
            color::detection_srgb8(&prepared.decoded.image, prepared.color).map_err(failed)?;
        let pixels = prepared.pixels();
        let found = self.detect(ctx, copy, pixels)?;
        if found.reran_on_cpu {
            ctx.notes.add(Note::AnalyzedAgainOnCpu);
        }
        ctx.progress.set(pixels, 2 * pixels);
        let evidence = found
            .over_limit
            .iter()
            .filter(|&&(_, dropped)| dropped > 0)
            .map(|&(model, dropped)| {
                let kept = found
                    .detections
                    .iter()
                    .filter(|d| d.model() == model)
                    .count() as u32;
                CoverageEvidence::unresolved(Problem::CappedCandidates {
                    model,
                    frame: None,
                    kept,
                    found: kept + dropped,
                })
            })
            .collect();
        let analysis = Analysis {
            detections: found.detections,
            evidence,
            source: Some(prepared.source.stamp),
            input: Some(content_digest(&prepared.source.bytes)),
        };
        Ok((analysis, prepared))
    }

    fn render(
        &self,
        ctx: &ItemContext<'_>,
        request: &RenderRequest<'_>,
        prepared: Option<Prepared>,
    ) -> Result<Rendered, StepError> {
        let mut prepared = match prepared {
            Some(p) => p,
            None => {
                let p = self.prepare(ctx)?;
                if request.analysis.source != Some(p.source.stamp)
                    || request.analysis.input != Some(content_digest(&p.source.bytes))
                {
                    return Err(failed(PhotoError::ChangedSinceAnalysis));
                }
                p
            }
        };
        let pixels = prepared.pixels();
        ctx.progress.set(pixels, 2 * pixels);
        let format = output_format(ctx.destination, prepared.source.format);
        if format == ImageFormat::Bmp {
            prepared.color =
                color::srgb_for_bmp(&mut prepared.decoded.image, prepared.color).map_err(failed)?;
        }
        match prepared.color {
            ColorOutcome::Converted => ctx.notes.add(Note::ColorConverted),
            ColorOutcome::Ignored(_) => ctx.notes.add(Note::ColorProfileIgnored),
            ColorOutcome::Untagged | ColorOutcome::Kept(_) => {}
        }

        let regions = if request.basis == PublishBasis::ExportedWithoutAnonymization {
            Vec::new()
        } else {
            if request.analysis.detections.is_empty() {
                ctx.notes.add(Note::NothingDetected);
            }
            request.edits.regions(&request.analysis.detections)
        };
        let overlay = self
            .settings
            .overlay
            .as_ref()
            .map(|o| color::overlay_for(o, prepared.color))
            .transpose()
            .map_err(failed)?;
        let image = &mut prepared.decoded.image;
        mask::apply(image, &regions, &self.settings.style, overlay.as_deref()).map_err(failed)?;
        checkpoint(ctx)?;
        let bytes = encode::encode(
            &prepared.source,
            image,
            prepared.color,
            self.settings.metadata,
            format,
        )
        .map_err(failed)?;
        verify(&bytes, format, (image.width(), image.height())).map_err(failed)?;
        let source = prepared.source.stamp;
        drop(prepared);

        let mut output = TempOutput::create(ctx.destination).map_err(failed)?;
        output.file().write_all(&bytes).map_err(failed)?;
        ctx.progress.set(2 * pixels, 2 * pixels);
        Ok(Rendered { output, source })
    }

    fn publish(
        &self,
        ctx: &ItemContext<'_>,
        rendered: Rendered,
        permit: &PublishPermit,
    ) -> Result<(), StepError> {
        if SourceStamp::of(ctx.input).ok() != Some(rendered.source) {
            return Err(failed(PhotoError::ChangedWhileProcessing));
        }
        match rendered.output.publish(permit) {
            Ok(published) => {
                for w in &published.warnings {
                    ctx.notes.add(w.note());
                }
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                Err(failed(PhotoError::NameTaken))
            }
            Err(e) => Err(failed(e)),
        }
    }

    fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            models: self.settings.models.digest(),
            settings: self.settings.digest(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::models::FaceModel;
    use cloakframe_core::detection::KindThresholds;
    use cloakframe_core::mask::MosaicStrength;

    fn with_plates() -> PhotoSettings {
        let models = DetectionModels::new(Some(FaceModel::Yolo5FaceN), true).unwrap();
        PhotoSettings {
            models,
            thresholds: models.default_thresholds(),
            ..PhotoSettings::default()
        }
    }

    #[test]
    fn every_setting_changes_the_fingerprint() {
        let base = with_plates();
        let processor = |s: PhotoSettings| PhotoProcessor::new(s).unwrap().fingerprint();
        let changed = [
            PhotoSettings {
                thresholds: Thresholds {
                    face: KindThresholds::new(0.25, 0.4).unwrap(),
                    ..base.thresholds
                },
                ..base.clone()
            },
            PhotoSettings {
                thresholds: Thresholds {
                    plate: KindThresholds::new(0.5, 0.3).unwrap(),
                    ..base.thresholds
                },
                ..base.clone()
            },
            PhotoSettings {
                style: MaskStyle {
                    method: Method::Mosaic,
                    ..MaskStyle::default()
                },
                ..base.clone()
            },
            PhotoSettings {
                style: MaskStyle {
                    mosaic_strength: MosaicStrength::new(12).unwrap(),
                    ..MaskStyle::default()
                },
                ..base.clone()
            },
            PhotoSettings {
                style: MaskStyle {
                    soft_edges: true,
                    ..MaskStyle::default()
                },
                ..base.clone()
            },
            PhotoSettings {
                metadata: MetadataPolicy::Allowlist { gps: false },
                ..base.clone()
            },
            PhotoSettings {
                metadata: MetadataPolicy::Allowlist { gps: true },
                ..base.clone()
            },
            PhotoSettings {
                overlay: Some(Overlay::new(1, 1, vec![0, 0, 0, 255]).unwrap()),
                ..base.clone()
            },
            PhotoSettings {
                tiles: false,
                ..base.clone()
            },
        ];
        let reference = processor(base.clone());
        assert_eq!(reference, processor(base));
        let mut seen = vec![reference.settings];
        for settings in changed {
            let fingerprint = processor(settings);
            assert_eq!(fingerprint.models, reference.models);
            assert!(!seen.contains(&fingerprint.settings));
            seen.push(fingerprint.settings);
        }
    }

    #[test]
    fn the_models_make_the_other_half_of_the_fingerprint() {
        let fingerprint = |s: PhotoSettings| PhotoProcessor::new(s).unwrap().fingerprint();
        let default = fingerprint(PhotoSettings::default());
        let plates = fingerprint(with_plates());
        assert_eq!(default.models, DetectionModels::default().digest());
        assert_eq!(plates.models, with_plates().models.digest());
        assert_ne!(default.models, plates.models);
        // Without plates, the plate thresholds decide nothing.
        let other_plate = PhotoSettings {
            thresholds: Thresholds {
                plate: KindThresholds::new(0.2, 0.3).unwrap(),
                ..PhotoSettings::default().thresholds
            },
            ..PhotoSettings::default()
        };
        assert_eq!(fingerprint(other_plate), default);
    }

    #[test]
    fn a_custom_image_method_needs_the_image() {
        let settings = PhotoSettings {
            style: MaskStyle {
                method: Method::CustomImage,
                ..MaskStyle::default()
            },
            ..PhotoSettings::default()
        };
        assert!(matches!(
            PhotoProcessor::new(settings),
            Err(MaskError::MissingOverlay)
        ));
    }

    #[test]
    fn the_output_name_decides_the_format() {
        let png = ImageFormat::Png;
        assert_eq!(output_format(Path::new("a/b.JPG"), png), ImageFormat::Jpeg);
        assert_eq!(output_format(Path::new("b.tif"), png), ImageFormat::Tiff);
        assert_eq!(output_format(Path::new("b.png"), ImageFormat::Jpeg), png);
        assert_eq!(output_format(Path::new("b"), png), png);
    }
}
