//! A photo drawn at any zoom up to past its full resolution. It is decoded once on a worker
//! thread, and only the tiles on screen are cut, at the level of detail the zoom needs, and
//! uploaded as textures: one texture of the whole photo would pass the GPU's limit (8192 on
//! wgpu by default, docs/M0_EGUI.md) and its memory. Only the most recent tiles are kept.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use cloakframe_core::mask::{MaskStyle, Region};
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::photo::budget::MemoryBudget;
use cloakframe_engine::photo::decode::{decode, read_source};
use cloakframe_engine::photo::mask;
use eframe::egui;
use image::{DynamicImage, RgbImage};

/// Side of a tile in pixels of its level.
pub const TILE: u32 = 512;
/// About 100 MB of textures.
const KEPT: usize = 96;

/// A tile of the photo at `level`, whose pixels each average 2^level × 2^level pixels of the
/// photo; `masked` names the masking preview it shows, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub level: u8,
    pub x: u32,
    pub y: u32,
    pub masked: Option<u64>,
}

/// The coarsest level, whose one tile holds the whole photo.
pub fn top_level((width, height): (u32, u32)) -> u8 {
    let mut level = 0;
    while width.max(height).div_ceil(1 << level) > TILE {
        level += 1;
    }
    level
}

/// The level whose pixels are the closest to, and no smaller than, the screen's at `scale`
/// physical pixels per photo pixel.
pub fn level_for(scale: f32, top: u8) -> u8 {
    if scale >= 1.0 || scale <= 0.0 {
        return 0;
    }
    ((1.0 / scale).log2().floor() as u8).min(top)
}

/// The photo's pixels a tile covers, as (left, top, right, bottom).
pub fn tile_area((width, height): (u32, u32), key: &TileKey) -> (u32, u32, u32, u32) {
    let span = TILE << key.level;
    let left = key.x * span;
    let top = key.y * span;
    (
        left,
        top,
        (left + span).min(width),
        (top + span).min(height),
    )
}

/// The tiles of `level` that meet the photo's area from `min` to `max`, in photo pixels.
pub fn tiles_in(
    size: (u32, u32),
    level: u8,
    min: (f32, f32),
    max: (f32, f32),
) -> impl Iterator<Item = (u32, u32)> {
    let span = (TILE << level) as f32;
    let columns = size.0.div_ceil(TILE << level);
    let rows = size.1.div_ceil(TILE << level);
    let first = |v: f32| (v.max(0.0) / span).floor() as u32;
    let last = |v: f32, count: u32| ((v.max(0.0) / span).ceil() as u32).min(count);
    let (x0, x1) = (first(min.0), last(max.0, columns));
    let (y0, y1) = (first(min.1), last(max.1, rows));
    (y0..y1).flat_map(move |y| (x0..x1).map(move |x| (x, y)))
}

/// The tile's pixels, each the mean of the photo pixels it covers.
pub fn cut(image: &RgbImage, key: &TileKey) -> egui::ColorImage {
    let size = image.dimensions();
    let (left, top, right, bottom) = tile_area(size, key);
    let factor = 1u32 << key.level;
    let width = (right - left).div_ceil(factor);
    let height = (bottom - top).div_ceil(factor);
    let mut pixels = Vec::with_capacity((width * height) as usize);
    for ty in 0..height {
        let y0 = top + ty * factor;
        let y1 = (y0 + factor).min(bottom);
        for tx in 0..width {
            let x0 = left + tx * factor;
            let x1 = (x0 + factor).min(right);
            let mut sum = [0u32; 3];
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = image.get_pixel(x, y).0;
                    sum[0] += u32::from(p[0]);
                    sum[1] += u32::from(p[1]);
                    sum[2] += u32::from(p[2]);
                }
            }
            let n = (x1 - x0) * (y1 - y0);
            let mean = |s: u32| ((s + n / 2) / n) as u8;
            pixels.push(egui::Color32::from_rgb(
                mean(sum[0]),
                mean(sum[1]),
                mean(sum[2]),
            ));
        }
    }
    egui::ColorImage::new([width as usize, height as usize], pixels)
}

enum Job {
    Tile(TileKey),
    Mask {
        generation: u64,
        regions: Vec<Region>,
        style: MaskStyle,
    },
}

enum Made {
    Size(u32, u32),
    Failed(String),
    /// `None` for a masking preview that has been replaced.
    Tile(TileKey, Option<egui::ColorImage>),
}

pub struct Picture {
    pub path: PathBuf,
    jobs: mpsc::Sender<Job>,
    made: mpsc::Receiver<Made>,
    cancel: CancelToken,
    pub size: Option<(u32, u32)>,
    pub error: Option<String>,
    textures: HashMap<TileKey, egui::TextureHandle>,
    order: VecDeque<TileKey>,
    asked: HashSet<TileKey>,
    /// The masking preview last asked for.
    masked: Option<u64>,
}

impl Picture {
    pub fn open(path: &Path, ctx: &egui::Context) -> std::io::Result<Self> {
        let (jobs, incoming) = mpsc::channel();
        let (send, made) = mpsc::channel();
        let cancel = CancelToken::new();
        let (token, repaint, source) = (cancel.clone(), ctx.clone(), path.to_path_buf());
        std::thread::Builder::new()
            .name("cloakframe-picture".into())
            .spawn(move || {
                let tell = |made| {
                    let sent = send.send(made).is_ok();
                    repaint.request_repaint();
                    sent
                };
                let budget = MemoryBudget::for_this_computer();
                let decoded = match read_source(&source).and_then(|s| decode(&s, &budget, &token)) {
                    Ok(decoded) => decoded,
                    Err(e) => {
                        tell(Made::Failed(e.to_string()));
                        return;
                    }
                };
                // The permit stays while the pixels do.
                let _permit = decoded.permit;
                let base = match decoded.image {
                    DynamicImage::ImageRgb8(rgb) => rgb,
                    other => other.to_rgb8(),
                };
                if !tell(Made::Size(base.width(), base.height())) {
                    return;
                }
                let mut masked: Option<(u64, RgbImage)> = None;
                for job in incoming {
                    if token.is_cancelled() {
                        return;
                    }
                    let made = match job {
                        Job::Tile(key) => match key.masked {
                            None => Made::Tile(key, Some(cut(&base, &key))),
                            Some(g) => match &masked {
                                Some((current, image)) if *current == g => {
                                    Made::Tile(key, Some(cut(image, &key)))
                                }
                                _ => Made::Tile(key, None),
                            },
                        },
                        Job::Mask {
                            generation,
                            regions,
                            style,
                        } => {
                            let mut image = DynamicImage::ImageRgb8(base.clone());
                            match mask::apply(&mut image, &regions, &style, None) {
                                Ok(()) => masked = Some((generation, image.into_rgb8())),
                                Err(e) => {
                                    tell(Made::Failed(e.to_string()));
                                    return;
                                }
                            }
                            continue;
                        }
                    };
                    if !tell(made) {
                        return;
                    }
                }
            })?;
        Ok(Self {
            path: path.to_path_buf(),
            jobs,
            made,
            cancel,
            size: None,
            error: None,
            textures: HashMap::new(),
            order: VecDeque::new(),
            asked: HashSet::new(),
            masked: None,
        })
    }

    /// Uploads what the worker made since the last frame.
    pub fn take(&mut self, ctx: &egui::Context) {
        while let Ok(made) = self.made.try_recv() {
            self.made_now(made, ctx);
        }
    }

    /// Waits until the photo is decoded and every tile asked for is made, for tests.
    pub fn wait(&mut self, ctx: &egui::Context) {
        while (self.size.is_none() && self.error.is_none()) || !self.asked.is_empty() {
            match self.made.recv() {
                Ok(made) => self.made_now(made, ctx),
                Err(_) => return,
            }
        }
    }

    fn made_now(&mut self, made: Made, ctx: &egui::Context) {
        match made {
            Made::Size(w, h) => self.size = Some((w, h)),
            Made::Failed(e) => {
                self.error = Some(e);
                self.asked.clear();
            }
            Made::Tile(key, image) => {
                self.asked.remove(&key);
                let Some(image) = image else {
                    return;
                };
                // Past full resolution, each photo pixel shows as a square.
                let options = if key.level == 0 {
                    egui::TextureOptions {
                        magnification: egui::TextureFilter::Nearest,
                        ..egui::TextureOptions::LINEAR
                    }
                } else {
                    egui::TextureOptions::LINEAR
                };
                let name = format!("{}#{key:?}", self.path.display());
                self.textures
                    .insert(key, ctx.load_texture(name, image, options));
                self.order.push_back(key);
                while self.order.len() > KEPT {
                    if let Some(old) = self.order.pop_front() {
                        self.textures.remove(&old);
                    }
                }
            }
        }
    }

    /// Asks for the masking preview `generation` of `regions`, unless it is the last one asked.
    pub fn mask(&mut self, generation: u64, regions: Vec<Region>, style: MaskStyle) {
        if self.masked == Some(generation) {
            return;
        }
        self.masked = Some(generation);
        self.textures.retain(|k, _| k.masked.is_none());
        self.order.retain(|k| k.masked.is_none());
        let _ = self.jobs.send(Job::Mask {
            generation,
            regions,
            style,
        });
    }

    fn ask(&mut self, key: TileKey) {
        if !self.textures.contains_key(&key) && self.error.is_none() && self.asked.insert(key) {
            let _ = self.jobs.send(Job::Tile(key));
        }
    }

    /// Draws the photo with its top left corner at `origin` and `scale` points per photo pixel,
    /// within `clip`. A tile not made yet is drawn from a coarser one that is.
    pub fn paint(
        &mut self,
        painter: &egui::Painter,
        origin: egui::Pos2,
        scale: f32,
        masked: Option<u64>,
    ) {
        let Some(size) = self.size else {
            return;
        };
        let clip = painter.clip_rect();
        let top = top_level(size);
        let level = level_for(scale * painter.pixels_per_point(), top);
        self.ask(TileKey {
            level: top,
            x: 0,
            y: 0,
            masked,
        });
        let min = ((clip.min - origin) / scale).to_pos2();
        let max = ((clip.max - origin) / scale).to_pos2();
        let screen = |(l, t, r, b): (u32, u32, u32, u32)| {
            egui::Rect::from_min_max(
                origin + egui::vec2(l as f32, t as f32) * scale,
                origin + egui::vec2(r as f32, b as f32) * scale,
            )
        };
        for (x, y) in tiles_in(size, level, (min.x, min.y), (max.x, max.y)).collect::<Vec<_>>() {
            let key = TileKey {
                level,
                x,
                y,
                masked,
            };
            self.ask(key);
            let area = tile_area(size, &key);
            let shown = (level..=top).find_map(|l| {
                let shift = l - level;
                let parent = TileKey {
                    level: l,
                    x: x >> shift,
                    y: y >> shift,
                    masked,
                };
                self.textures
                    .get(&parent)
                    .map(|t| (t, tile_area(size, &parent)))
            });
            if let Some((texture, (pl, pt, pr, pb))) = shown {
                let (w, h) = ((pr - pl) as f32, (pb - pt) as f32);
                let uv = egui::Rect::from_min_max(
                    egui::pos2((area.0 - pl) as f32 / w, (area.1 - pt) as f32 / h),
                    egui::pos2((area.2 - pl) as f32 / w, (area.3 - pt) as f32 / h),
                );
                painter.image(texture.id(), screen(area), uv, egui::Color32::WHITE);
            }
        }
    }

    pub fn is_loading(&self) -> bool {
        (self.size.is_none() && self.error.is_none()) || !self.asked.is_empty()
    }
}

impl Drop for Picture {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloakframe_core::geometry::Rect;

    fn key(level: u8, x: u32, y: u32) -> TileKey {
        TileKey {
            level,
            x,
            y,
            masked: None,
        }
    }

    #[test]
    fn levels_and_tiles_cover_a_photo_past_the_texture_limit() {
        let size = (11648, 8736);
        assert_eq!(top_level(size), 5);
        assert_eq!(top_level((512, 300)), 0);
        assert_eq!(level_for(2.0, 5), 0);
        assert_eq!(level_for(1.0, 5), 0);
        assert_eq!(level_for(0.6, 5), 0);
        assert_eq!(level_for(0.5, 5), 1);
        assert_eq!(level_for(0.03, 5), 5);
        assert_eq!(tile_area(size, &key(0, 22, 17)), (11264, 8704, 11648, 8736));
        assert_eq!(tile_area(size, &key(5, 0, 0)), (0, 0, 11648, 8736));
        let all: Vec<_> = tiles_in(size, 0, (0.0, 0.0), (11648.0, 8736.0)).collect();
        assert_eq!(all.len(), 23 * 18);
        let some: Vec<_> = tiles_in(size, 1, (1000.0, -50.0), (2100.0, 1100.0)).collect();
        assert_eq!(some, [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)]);
        assert_eq!(
            tiles_in(size, 0, (12000.0, 0.0), (13000.0, 10.0)).count(),
            0
        );
    }

    /// A photo wider than the GPU's texture limit is shown in tiles no larger than one, and the
    /// masking preview's tiles come from the masked pixels.
    #[test]
    fn a_wide_photo_is_tiled_and_previewed_masked() {
        let dir = std::env::temp_dir().join(format!("cloakframe-tiles-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wide.png");
        RgbImage::from_pixel(9000, 600, image::Rgb([200, 180, 160]))
            .save(&path)
            .unwrap();
        let ctx = egui::Context::default();
        let mut picture = Picture::open(&path, &ctx).unwrap();
        picture.wait(&ctx);
        assert_eq!(picture.size, Some((9000, 600)));
        let columns = 9000u32.div_ceil(TILE);
        for x in 0..columns {
            picture.ask(key(0, x, 1));
        }
        picture.wait(&ctx);
        let widths: u32 = (0..columns)
            .map(|x| {
                let [w, h] = picture.textures[&key(0, x, 1)].size();
                assert!(w <= TILE as usize && h == 600 - TILE as usize);
                w as u32
            })
            .sum();
        assert_eq!(widths, 9000);

        let face = Rect::new(100.0, 100.0, 50.0, 50.0).unwrap();
        picture.mask(1, vec![Region::new(face)], MaskStyle::default());
        let masked = TileKey {
            masked: Some(1),
            ..key(0, 0, 0)
        };
        picture.ask(masked);
        let Ok(Made::Tile(made, Some(image))) = picture.made.recv() else {
            panic!("a masked tile");
        };
        assert_eq!(made, masked);
        let base = egui::Color32::from_rgb(200, 180, 160);
        assert_ne!(
            image.pixels[125 * image.size[0] + 125],
            base,
            "inside the region"
        );
        assert_eq!(image.pixels[300 * image.size[0] + 300], base, "outside it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_coarse_tile_averages_the_pixels_it_covers() {
        let image = RgbImage::from_fn(5, 3, |x, _| {
            if x % 2 == 0 {
                image::Rgb([0, 100, 255])
            } else {
                image::Rgb([200, 100, 0])
            }
        });
        let full = cut(&image, &key(0, 0, 0));
        assert_eq!(full.size, [5, 3]);
        assert_eq!(full.pixels[1], egui::Color32::from_rgb(200, 100, 0));
        let half = cut(&image, &key(1, 0, 0));
        assert_eq!(half.size, [3, 2]);
        assert_eq!(half.pixels[0], egui::Color32::from_rgb(100, 100, 128));
        assert_eq!(
            half.pixels[2],
            egui::Color32::from_rgb(0, 100, 255),
            "the edge column alone"
        );
    }
}
