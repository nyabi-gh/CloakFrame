//! Photos and videos in one run: each item goes to the pipeline its kind needs.

use std::path::Path;

use cloakframe_core::item::PublishPermit;

use crate::cancel::CancelToken;
use crate::photo::process::{self as photo, PhotoProcessor};
use crate::run::{
    Analysis, FailureKind, Fingerprint, ItemContext, ItemProcessor, RenderRequest, StepError,
};
use crate::scan::MediaKind;
use crate::video::process::{self as video, VideoProcessor};

pub struct MediaProcessor {
    photo: PhotoProcessor,
    video: Option<VideoProcessor>,
}

impl MediaProcessor {
    /// Without a video processor, video items fail.
    pub fn new(photo: PhotoProcessor, video: Option<VideoProcessor>) -> Self {
        Self { photo, video }
    }

    /// The work an item's progress will count ([`crate::events::Progress`]), read from the start
    /// of the file before the run reaches it. `None` when that does not tell.
    pub fn estimated_work(&self, input: &Path, cancel: &CancelToken) -> Option<u64> {
        match MediaKind::of(input)? {
            MediaKind::Image => {
                let (width, height) = crate::photo::decode::start_dimensions(input)?;
                Some(2 * u64::from(width) * u64::from(height))
            }
            MediaKind::Video => self.video.as_ref()?.estimated_work(input, cancel),
        }
    }

    /// The video processor, which the preview of a video under review opens with.
    pub fn video(&self) -> Result<&VideoProcessor, StepError> {
        self.video.as_ref().ok_or_else(|| {
            StepError::failed(FailureKind::Unsupported, "this run does not process videos")
        })
    }
}

fn is_video(ctx: &ItemContext<'_>) -> bool {
    MediaKind::of(ctx.input) == Some(MediaKind::Video)
}

pub enum Prepared {
    Photo(photo::Prepared),
    Video(video::Analyzed),
}

pub enum Rendered {
    Photo(photo::Rendered),
    Video(video::Rendered),
}

impl ItemProcessor for MediaProcessor {
    type Prepared = Prepared;
    type Rendered = Rendered;

    fn analyze(&self, ctx: &ItemContext<'_>) -> Result<(Analysis, Prepared), StepError> {
        if is_video(ctx) {
            let (analysis, prepared) = self.video()?.analyze(ctx)?;
            Ok((analysis, Prepared::Video(prepared)))
        } else {
            let (analysis, prepared) = self.photo.analyze(ctx)?;
            Ok((analysis, Prepared::Photo(prepared)))
        }
    }

    fn render(
        &self,
        ctx: &ItemContext<'_>,
        request: &RenderRequest<'_>,
        prepared: Option<Prepared>,
    ) -> Result<Rendered, StepError> {
        match prepared {
            Some(Prepared::Video(p)) => Ok(Rendered::Video(self.video()?.render(
                ctx,
                request,
                Some(p),
            )?)),
            Some(Prepared::Photo(p)) => {
                Ok(Rendered::Photo(self.photo.render(ctx, request, Some(p))?))
            }
            None if is_video(ctx) => Ok(Rendered::Video(self.video()?.render(ctx, request, None)?)),
            None => Ok(Rendered::Photo(self.photo.render(ctx, request, None)?)),
        }
    }

    fn publish(
        &self,
        ctx: &ItemContext<'_>,
        rendered: Rendered,
        permit: &PublishPermit,
    ) -> Result<(), StepError> {
        match rendered {
            Rendered::Photo(r) => self.photo.publish(ctx, r, permit),
            Rendered::Video(r) => self.video()?.publish(ctx, r, permit),
        }
    }

    /// The pipeline's that processes `input`, so the video settings do not refuse a review of
    /// the photos.
    fn fingerprint(&self, input: &Path) -> Fingerprint {
        match &self.video {
            Some(video) if MediaKind::of(input) == Some(MediaKind::Video) => {
                video.fingerprint(input)
            }
            _ => self.photo.fingerprint(input),
        }
    }
}
