//! Photos and videos in one run: each item goes to the pipeline its kind needs.

use cloakframe_core::item::PublishPermit;

use crate::photo::process::{self as photo, PhotoProcessor};
use crate::run::{Analysis, Fingerprint, ItemContext, ItemProcessor, RenderRequest, StepError};
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

    fn video(&self) -> Result<&VideoProcessor, StepError> {
        self.video
            .as_ref()
            .ok_or_else(|| StepError::Failed("this run does not process videos".into()))
    }
}

fn is_video(ctx: &ItemContext<'_>) -> bool {
    MediaKind::of(ctx.input) == Some(MediaKind::Video)
}

pub enum Prepared {
    Photo(photo::Prepared),
    Video(crate::media::source::Source),
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

    /// The photo pipeline's: only photos are kept for review (videos join with M5), so the
    /// video settings must not refuse a review of the run's photos.
    fn fingerprint(&self) -> Fingerprint {
        self.photo.fingerprint()
    }
}
