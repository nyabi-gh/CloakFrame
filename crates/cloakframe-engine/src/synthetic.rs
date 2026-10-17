//! A processor that exercises the whole run without a media pipeline. It reads the input once,
//! "renders" the bytes reversed (never a copy of the source, D9) and reports problems named in the
//! file stem: `gap` gives an unresolved gap, `capped` capped candidates, `fail` a decode failure.

use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::Mutex;

use cloakframe_core::coverage::{CoverageEvidence, Problem};
use cloakframe_core::detection::ModelId;
use cloakframe_core::item::{ItemId, PublishPermit};
use cloakframe_core::review::Digest;
use sha2::{Digest as _, Sha256};

use crate::identity::SourceStamp;
use crate::publish::TempOutput;
use crate::run::{Analysis, Fingerprint, ItemContext, ItemProcessor, StepError};

#[derive(Default)]
pub struct SyntheticProcessor {
    rendered: Mutex<HashMap<ItemId, (TempOutput, SourceStamp)>>,
}

fn failed(message: &str) -> StepError {
    StepError::Failed(message.into())
}

fn io_failed(e: io::Error) -> StepError {
    StepError::Failed(Box::new(e))
}

impl ItemProcessor for SyntheticProcessor {
    fn analyze(&self, ctx: &ItemContext<'_>) -> Result<Analysis, StepError> {
        let stem = ctx
            .input
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if stem.contains("fail") {
            return Err(failed("synthetic decode failure"));
        }
        let mut evidence = Vec::new();
        if stem.contains("gap") {
            evidence.push(CoverageEvidence::unresolved(Problem::UnresolvedGap {
                track: 1,
                first: 30,
                last: 54,
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
        Ok(Analysis {
            detections: Vec::new(),
            evidence,
        })
    }

    fn render(&self, ctx: &ItemContext<'_>, _: &Analysis) -> Result<(), StepError> {
        let stamp = SourceStamp::of(ctx.input).map_err(io_failed)?;
        let mut bytes = std::fs::read(ctx.input).map_err(io_failed)?;
        bytes.reverse();
        let mut out = TempOutput::create(ctx.destination).map_err(io_failed)?;
        out.file().write_all(&bytes).map_err(io_failed)?;
        ctx.progress.set(1, 1);
        self.rendered
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(ctx.item, (out, stamp));
        Ok(())
    }

    fn publish(&self, ctx: &ItemContext<'_>, permit: &PublishPermit) -> Result<(), StepError> {
        let (out, stamp) = self
            .rendered
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&ctx.item)
            .ok_or_else(|| failed("nothing was rendered"))?;
        if SourceStamp::of(ctx.input).ok() != Some(stamp) {
            return Err(failed("the source changed while it was processed"));
        }
        match out.publish(permit) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                Err(failed("the output name was taken during the run"))
            }
            Err(e) => Err(io_failed(e)),
        }
    }

    fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            models: Digest(Sha256::digest(b"synthetic").into()),
            settings: Digest(Sha256::digest(b"").into()),
        }
    }
}
