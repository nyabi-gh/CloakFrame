//! A processor that exercises the whole run without a media pipeline. It reads the input once,
//! "renders" the bytes reversed (never a copy of the source, D9) and reports problems named in the
//! file stem: `gap` gives an unresolved gap, `capped` capped candidates, `fail` a decode failure.
//! `slow` keeps its temp output open until cancelled and `stuck` for a minute regardless, for the
//! CLI's Ctrl+C tests. [`FixedDetection`] stands in for the models in the real pipelines.

use std::io::{self, Write};
use std::time::{Duration, Instant};

use cloakframe_core::coverage::{CoverageEvidence, Problem};
use cloakframe_core::detection::{Detection, DetectionKind, ModelId, Thresholds};
use cloakframe_core::geometry::Rect;
use cloakframe_core::item::PublishPermit;
use cloakframe_core::review::Digest;
use sha2::{Digest as _, Sha256};

use crate::detect::{DetectError, Detector, Found, Rgb8Image};
use crate::identity::{SourceStamp, content_digest};
use crate::publish::TempOutput;
use crate::run::{Analysis, Fingerprint, ItemContext, ItemProcessor, RenderRequest, StepError};

#[derive(Default)]
pub struct SyntheticProcessor;

/// Finds one face, this box in the pixels of the image it is given, wherever the box overlaps
/// that image. With tiles the same box is found in every tile, so its tests detect without them.
pub struct FixedDetection(pub Rect);

impl Detector for FixedDetection {
    fn detect(&mut self, image: &Rgb8Image, _: &Thresholds) -> Result<Found, DetectError> {
        let b = self.0;
        let overlaps = b.x() < image.width as f32
            && b.y() < image.height as f32
            && b.right() > 0.0
            && b.bottom() > 0.0;
        let detections = if overlaps {
            vec![Detection::new(ModelId::Yolo5FaceN, 0.9, b, None, None).expect("a valid face")]
        } else {
            Vec::new()
        };
        Ok(Found {
            detections,
            ..Found::default()
        })
    }
}

fn failed(message: &str) -> StepError {
    StepError::Failed(message.into())
}

fn io_failed(e: io::Error) -> StepError {
    StepError::Failed(Box::new(e))
}

fn stem(ctx: &ItemContext<'_>) -> String {
    ctx.input
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

impl ItemProcessor for SyntheticProcessor {
    type Prepared = ();
    type Rendered = (TempOutput, SourceStamp);

    fn analyze(&self, ctx: &ItemContext<'_>) -> Result<(Analysis, ()), StepError> {
        let stem = stem(ctx);
        if stem.contains("fail") {
            return Err(failed("synthetic decode failure"));
        }
        let mut evidence = Vec::new();
        if stem.contains("gap") {
            evidence.push(CoverageEvidence::unresolved(Problem::UnresolvedGap {
                detection: DetectionKind::Face,
                tracks: vec![1],
                first: 30,
                last: 64,
            }));
        }
        if stem.contains("capped") {
            evidence.push(CoverageEvidence::unresolved(Problem::CappedCandidates {
                model: ModelId::Yolo5FaceN,
                frame: None,
                kept: 100,
                found: 280,
            }));
        }
        let source = Some(SourceStamp::of(ctx.input).map_err(io_failed)?);
        let input = Some(content_digest(
            &std::fs::read(ctx.input).map_err(io_failed)?,
        ));
        Ok((
            Analysis {
                detections: Vec::new(),
                evidence,
                source,
                input,
                video: None,
            },
            (),
        ))
    }

    fn render(
        &self,
        ctx: &ItemContext<'_>,
        request: &RenderRequest<'_>,
        _: Option<()>,
    ) -> Result<Self::Rendered, StepError> {
        let stamp = SourceStamp::of(ctx.input).map_err(io_failed)?;
        let mut bytes = std::fs::read(ctx.input).map_err(io_failed)?;
        if request.analysis.source != Some(stamp)
            || request.analysis.input != Some(content_digest(&bytes))
        {
            return Err(failed("the source changed since it was analyzed"));
        }
        bytes.reverse();
        let mut out = TempOutput::create(ctx.destination).map_err(io_failed)?;
        out.file().write_all(&bytes).map_err(io_failed)?;
        let stem = stem(ctx);
        if stem.contains("slow") || stem.contains("stuck") {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(60) {
                if stem.contains("slow") && ctx.cancel.is_cancelled() {
                    return Err(StepError::Cancelled);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        ctx.progress.set(1, 1);
        Ok((out, stamp))
    }

    fn publish(
        &self,
        ctx: &ItemContext<'_>,
        (out, stamp): Self::Rendered,
        permit: &PublishPermit,
    ) -> Result<(), StepError> {
        if SourceStamp::of(ctx.input).ok() != Some(stamp) {
            return Err(failed("the source changed while it was processed"));
        }
        match out.publish(permit) {
            Ok(published) => {
                for w in &published.warnings {
                    ctx.notes.add(w.note());
                }
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                Err(failed("the output name was taken during the run"))
            }
            Err(e) => Err(io_failed(e)),
        }
    }

    fn fingerprint(&self, _: &std::path::Path) -> Fingerprint {
        Fingerprint {
            models: Digest(Sha256::digest(b"synthetic").into()),
            settings: Digest(Sha256::digest(b"").into()),
        }
    }
}
