//! The main screen's sample photo with what and how applied to it as a run would (plan 6, main
//! screen 2). The face is marked by hand, so the preview needs no model and follows every change
//! at once; the sensitivity and the model, which decide what is found, do not show in it.

use cloakframe_core::geometry::Rect;
use cloakframe_core::mask::Region;
use cloakframe_engine::photo::mask;
use eframe::egui;
use image::{DynamicImage, ImageFormat, RgbImage};

use crate::run::RunOptions;

/// NASA's portrait of Eileen Collins (public domain), made by `tools/sample/make.sh`.
const PHOTO: &[u8] = include_bytes!("../assets/sample/astronaut.webp");

/// Her face in the 256×256 sample.
fn face() -> Rect {
    Rect::new(85.0, 20.0, 55.0, 75.0).expect("a valid box")
}

pub struct Sample {
    photo: RgbImage,
    /// The texture and the options it was masked with.
    shown: Option<(RunOptions, egui::TextureHandle)>,
}

impl Sample {
    pub fn new() -> Self {
        let photo = image::load_from_memory_with_format(PHOTO, ImageFormat::WebP)
            .expect("the bundled sample decodes")
            .into_rgb8();
        Self { photo, shown: None }
    }

    /// The sample as a run with `options` would save it.
    pub fn masked(&self, options: &RunOptions) -> RgbImage {
        let mut image = DynamicImage::ImageRgb8(self.photo.clone());
        if options.faces {
            mask::apply(&mut image, &[Region::new(face())], &options.style(), None)
                .expect("an 8-bit RGB photo masks");
        }
        image.into_rgb8()
    }

    /// Made again only when what or how changed.
    pub fn texture(&mut self, ctx: &egui::Context, options: &RunOptions) -> &egui::TextureHandle {
        let looks_the_same = |shown: &RunOptions| {
            shown.faces == options.faces
                && shown.method == options.method
                && shown.mosaic_strength == options.mosaic_strength
        };
        if !self.shown.as_ref().is_some_and(|(o, _)| looks_the_same(o)) {
            let image = self.masked(options);
            let size = [image.width() as usize, image.height() as usize];
            let pixels = egui::ColorImage::from_rgb(size, image.as_raw());
            let texture = ctx.load_texture("sample", pixels, egui::TextureOptions::LINEAR);
            self.shown = Some((*options, texture));
        }
        &self.shown.as_ref().expect("just made").1
    }
}

impl Default for Sample {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Method, Settings};

    fn options(faces: bool, method: Method, strength: u8) -> RunOptions {
        RunOptions {
            faces,
            method,
            mosaic_strength: strength,
            ..RunOptions::of(&Settings::default())
        }
    }

    /// Only the face changes, and each method and strength changes it differently; with faces
    /// off nothing does.
    #[test]
    fn what_and_how_change_only_the_face() {
        let sample = Sample::new();
        let original = sample.masked(&options(false, Method::Fill, 8));
        assert_eq!(original, sample.photo);
        let face = face();
        let inside = |x: u32, y: u32| {
            let (x, y) = (x as f32, y as f32);
            x >= face.x() - 20.0
                && x <= face.x() + face.width() + 20.0
                && y >= face.y() - 20.0
                && y <= face.y() + face.height() + 20.0
        };
        let mut seen = Vec::new();
        for (method, strength) in [
            (Method::Fill, 8),
            (Method::Mosaic, 4),
            (Method::Mosaic, 12),
            (Method::Blur, 8),
        ] {
            let masked = sample.masked(&options(true, method, strength));
            let mut changed = 0;
            for (x, y, pixel) in masked.enumerate_pixels() {
                if pixel != original.get_pixel(x, y) {
                    assert!(inside(x, y), "{method:?} changed ({x}, {y})");
                    changed += 1;
                }
            }
            assert!(changed > 2000, "{method:?} changed only {changed} pixels");
            assert!(
                !seen.contains(&masked),
                "{method:?} {strength} looks like another"
            );
            seen.push(masked);
        }
    }
}
