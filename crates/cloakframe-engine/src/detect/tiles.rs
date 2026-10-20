//! Tile mode (plan 4.3): besides the whole view, the image is detected in overlapping tiles on
//! levels from the whole view's scale up to the source's own pixels, so faces too small for the
//! whole view are found (docs/M3_TILES.md).
//!
//! Every face between `MIN_FOUND` model pixels on the finest level and the whole view lies wholly
//! inside a tile of some level at `MIN_FOUND`..=`OVERLAP` model pixels, or is at least `MIN_FOUND`
//! in the whole view: adjacent levels differ by at most `OVERLAP / MIN_FOUND` in scale, and a box
//! no larger than the overlap always fits in one tile. So a tile's box larger than `MAX_IN_TILE`
//! is left to the coarser views, where the same thing is wholly seen; the plate model calls a
//! plain tile one large plate.

use cloakframe_core::detection::{
    Detection, DetectionKind, ModelId, Thresholds, non_max_suppression,
};
use cloakframe_core::geometry::{Point, Rect};

use super::resize::{Region, resize_region_linear};
use super::{Found, Rgb8Image};

/// A tile's side in model pixels: the largest input of the pinned models. The plate model scales
/// it to its 512.
pub const MODEL_SIDE: u32 = 640;
/// Adjacent tiles of a level share at least this many model pixels.
const OVERLAP: u32 = 160;
/// Faces about this tall in model pixels are found reliably by YOLO5Face-n and YuNet
/// (docs/M3_TILES.md); it sets how far apart the levels may be.
const MIN_FOUND: u32 = OVERLAP / 4;
/// A box cut by a tile edge is dropped when a whole box of its kind covers this share of it.
const COVERED: f32 = 0.8;
/// The largest box, in model pixels on either side, a tile's findings keep.
const MAX_IN_TILE: f32 = (MODEL_SIDE / 2) as f32;

/// One tile: a region of the image and the size the detector is given it at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tile {
    pub region: Region,
    pub input_width: u32,
    pub input_height: u32,
    /// Whether each side (left, top, right, bottom) lies inside the image rather than on its
    /// border; a box touching such a side may be cut.
    inner: [bool; 4],
}

impl Tile {
    /// The tile's pixels at its input size.
    pub fn image(&self, image: &Rgb8Image) -> Rgb8Image {
        resize_region_linear(image, self.region, self.input_width, self.input_height)
    }

    /// `d`, found in this tile's input, in image coordinates, and whether it touches an inner
    /// side and may be part of something the tile cut.
    fn locate(&self, d: &Detection) -> (Detection, bool) {
        let fx = self.region.width as f32 / self.input_width as f32;
        let fy = self.region.height as f32 / self.input_height as f32;
        let (x0, y0) = (self.region.x as f32, self.region.y as f32);
        let b = d.bbox();
        let bbox = Rect::new(
            x0 + b.x() * fx,
            y0 + b.y() * fy,
            b.width() * fx,
            b.height() * fy,
        )
        .expect("a scaled box stays finite and non-negative");
        let landmarks = d.landmarks().map(|points| {
            points.map(|p| Point {
                x: x0 + p.x * fx,
                y: y0 + p.y * fy,
            })
        });
        let moved = Detection::new(d.model(), d.score(), bbox, landmarks, d.frame())
            .expect("moving a detection keeps it valid");
        // Detectors clamp boxes to their input, so a cut box ends on the side; one input pixel
        // allows for rounding.
        let (w, h) = (self.input_width as f32, self.input_height as f32);
        let touches = [
            b.x() <= 1.0,
            b.y() <= 1.0,
            b.right() >= w - 1.0,
            b.bottom() >= h - 1.0,
        ];
        let cut = touches.iter().zip(self.inner).any(|(&t, inner)| t && inner);
        (moved, cut)
    }
}

/// The tiles of every level, coarsest first. Empty when the whole view already shows the image
/// at its own pixels or larger.
pub fn layout(width: u32, height: u32) -> Vec<Tile> {
    let side = f64::from(MODEL_SIDE);
    let whole = (side / f64::from(width)).min(side / f64::from(height));
    if whole >= 1.0 {
        return Vec::new();
    }
    let max_ratio = f64::from(OVERLAP / MIN_FOUND);
    let levels = ((1.0 / whole).ln() / max_ratio.ln()).ceil() as u32;
    let mut tiles = Vec::new();
    for level in 1..=levels {
        let scale = whole.powf(f64::from(levels - level) / f64::from(levels));
        let tile = (side / scale).round() as u32;
        let overlap = (f64::from(OVERLAP) / scale).ceil() as u32;
        let xs = starts(width, tile, overlap);
        let ys = starts(height, tile, overlap);
        for &(y, tile_h) in &ys {
            for &(x, tile_w) in &xs {
                let input = |len: u32| u64::from(len) * u64::from(MODEL_SIDE) / u64::from(tile);
                tiles.push(Tile {
                    region: Region {
                        x,
                        y,
                        width: tile_w,
                        height: tile_h,
                    },
                    input_width: input(tile_w).max(1) as u32,
                    input_height: input(tile_h).max(1) as u32,
                    inner: [x > 0, y > 0, x + tile_w < width, y + tile_h < height],
                });
            }
        }
    }
    tiles
}

/// Where tiles of `tile` pixels start along an axis of `len`, spread evenly so adjacent ones share
/// at least `overlap`, and their length. One tile of `len` when the axis is no longer than a tile.
fn starts(len: u32, tile: u32, overlap: u32) -> Vec<(u32, u32)> {
    if len <= tile {
        return vec![(0, len)];
    }
    let span = u64::from(len - tile);
    let count = (span.div_ceil(u64::from(tile - overlap)) + 1).max(2);
    (0..count)
        .map(|i| {
            let start = (2 * i * span + (count - 1)) / (2 * (count - 1));
            (start as u32, tile)
        })
        .collect()
}

/// The whole view's and the tiles' findings as one result. A tile's box larger than
/// `MAX_IN_TILE` is dropped (see the module comment). A box cut by a tile edge is dropped
/// when a whole box covers most of it, before NMS: a partial box can outscore the whole one and
/// NMS would then keep the part. The rest goes through each kind's NMS, faces before plates as
/// the whole view lists them. Dropped counts of bounded models add up over the views. Without
/// tiles the whole view is the result as it is.
pub fn merge(whole: Found, tiles: Vec<(Tile, Found)>, thresholds: &Thresholds) -> Found {
    if tiles.is_empty() {
        return whole;
    }
    let mut over_limit = whole.over_limit;
    let mut reran_on_cpu = whole.reran_on_cpu;
    let mut found: Vec<(Detection, bool)> =
        whole.detections.into_iter().map(|d| (d, false)).collect();
    for (tile, f) in tiles {
        found.extend(
            f.detections
                .iter()
                .filter(|d| d.bbox().width().max(d.bbox().height()) <= MAX_IN_TILE)
                .map(|d| tile.locate(d)),
        );
        over_limit.extend(f.over_limit);
        reran_on_cpu |= f.reran_on_cpu;
    }
    let complete: Vec<&Detection> = found
        .iter()
        .filter(|(_, cut)| !cut)
        .map(|(d, _)| d)
        .collect();
    let covered = |part: &Detection| {
        complete
            .iter()
            .any(|d| d.kind() == part.kind() && share_inside(&part.bbox(), &d.bbox()) >= COVERED)
    };
    let kept: Vec<Detection> = found
        .iter()
        .filter(|(d, cut)| !cut || !covered(d))
        .map(|(d, _)| d.clone())
        .collect();
    let mut detections = non_max_suppression(kept, thresholds);
    detections.sort_by_key(|d| d.kind() == DetectionKind::Plate);
    Found {
        detections,
        over_limit: sum_by_model(over_limit),
        reran_on_cpu,
    }
}

/// How much of `a`'s area lies inside `b`.
fn share_inside(a: &Rect, b: &Rect) -> f32 {
    let w = (a.right().min(b.right()) - a.x().max(b.x())).max(0.0);
    let h = (a.bottom().min(b.bottom()) - a.y().max(b.y())).max(0.0);
    if a.area() > 0.0 {
        w * h / a.area()
    } else {
        0.0
    }
}

fn sum_by_model(counts: Vec<(ModelId, u32)>) -> Vec<(ModelId, u32)> {
    let mut sums: Vec<(ModelId, u32)> = Vec::new();
    for (model, n) in counts {
        match sums.iter_mut().find(|(m, _)| *m == model) {
            Some((_, total)) => *total += n,
            None => sums.push((model, n)),
        }
    }
    sums
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloakframe_core::detection::KindThresholds;

    fn levels(tiles: &[Tile]) -> Vec<Vec<Tile>> {
        let mut out: Vec<Vec<Tile>> = Vec::new();
        for t in tiles {
            match out.last_mut() {
                Some(level) if scale(&level[0]) == scale(t) => level.push(*t),
                _ => out.push(vec![*t]),
            }
        }
        out
    }

    fn scale(t: &Tile) -> f64 {
        f64::from(MODEL_SIDE) / f64::from(t.region.width.max(t.region.height))
    }

    const SIZES: [(u32, u32); 9] = [
        (641, 480),
        (1920, 1080),
        (2560, 640),
        (4000, 3000),
        (3000, 4000),
        (6016, 6016),
        (8000, 600),
        (12_000, 9000),
        (24_000, 21_000),
    ];

    #[test]
    fn nothing_to_tile_when_the_whole_view_is_full_size() {
        for (w, h) in [(640, 640), (512, 300), (1, 1), (640, 1)] {
            assert!(layout(w, h).is_empty(), "{w}x{h}");
        }
    }

    #[test]
    fn levels_climb_to_the_source_pixels_in_steps_of_at_most_four() {
        for (w, h) in SIZES {
            let tiles = layout(w, h);
            let levels = levels(&tiles);
            let whole = (640.0 / f64::from(w)).min(640.0 / f64::from(h));
            let mut previous = whole;
            for level in &levels {
                let s = scale(&level[0]);
                assert!(s / previous <= 4.0 + 1e-6, "{w}x{h}: {previous} -> {s}");
                previous = s;
            }
            assert!((previous - 1.0).abs() < 1e-9, "{w}x{h}: finest {previous}");
        }
        assert_eq!(levels(&layout(1920, 1080)).len(), 1);
        assert_eq!(levels(&layout(4000, 3000)).len(), 2);
        assert_eq!(levels(&layout(24_000, 21_000)).len(), 3);
    }

    #[test]
    fn each_level_covers_the_image_with_overlapping_tiles() {
        for (w, h) in SIZES {
            for level in levels(&layout(w, h)) {
                let side = level[0].region.width.max(level[0].region.height);
                let overlap = (f64::from(OVERLAP) * f64::from(side) / 640.0).ceil() as u32;
                for t in &level {
                    let r = t.region;
                    assert!(r.x + r.width <= w && r.y + r.height <= h);
                    assert!(t.input_width <= MODEL_SIDE && t.input_height <= MODEL_SIDE);
                    assert!(t.input_width == MODEL_SIDE || t.input_height == MODEL_SIDE);
                    let on_border = [r.x == 0, r.y == 0, r.x + r.width == w, r.y + r.height == h];
                    assert_eq!(t.inner, on_border.map(|b| !b), "{w}x{h} {r:?}");
                }
                let mut xs: Vec<(u32, u32)> =
                    level.iter().map(|t| (t.region.x, t.region.width)).collect();
                xs.sort_unstable();
                xs.dedup();
                let mut ys: Vec<(u32, u32)> = level
                    .iter()
                    .map(|t| (t.region.y, t.region.height))
                    .collect();
                ys.sort_unstable();
                ys.dedup();
                for (axis, len) in [(&xs, w), (&ys, h)] {
                    assert_eq!(axis[0].0, 0);
                    let last = axis[axis.len() - 1];
                    assert_eq!(last.0 + last.1, len, "{w}x{h}");
                    for pair in axis.windows(2) {
                        assert!(
                            pair[0].0 + pair[0].1 >= pair[1].0 + overlap,
                            "{w}x{h} {pair:?}"
                        );
                    }
                }
                assert_eq!(xs.len() * ys.len(), level.len());
            }
        }
    }

    /// Every square box at least `MIN_FOUND` source pixels tall is at least that tall in the whole
    /// view or lies wholly inside a tile where it is `MIN_FOUND..=OVERLAP` model pixels.
    #[test]
    fn every_box_is_seen_whole_at_a_findable_size() {
        let mut seed = 0x2545_f491_u64;
        let mut next = |n: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % u64::from(n)) as u32
        };
        for (w, h) in SIZES {
            let tiles = layout(w, h);
            let whole = (640.0 / f64::from(w)).min(640.0 / f64::from(h));
            for _ in 0..2000 {
                let size = MIN_FOUND + next(w.min(h) - MIN_FOUND);
                let (x, y) = (next(w - size + 1), next(h - size + 1));
                if f64::from(size) * whole >= f64::from(MIN_FOUND) {
                    continue;
                }
                let seen = tiles.iter().any(|t| {
                    let r = t.region;
                    let model = f64::from(size) * scale(t);
                    r.x <= x
                        && r.y <= y
                        && x + size <= r.x + r.width
                        && y + size <= r.y + r.height
                        && model >= f64::from(MIN_FOUND) - 1e-6
                });
                assert!(seen, "{w}x{h}: {size} px at ({x}, {y})");
            }
        }
    }

    #[test]
    fn tile_inputs_resize_their_region() {
        let image = Rgb8Image {
            width: 2000,
            height: 700,
            data: (0..2000 * 700 * 3).map(|i| (i % 251) as u8).collect(),
        };
        let tiles = layout(2000, 700);
        let t = tiles[1];
        let input = t.image(&image);
        assert_eq!((input.width, input.height), (t.input_width, t.input_height));
        assert_eq!((t.input_width, t.input_height), (640, 640));
        assert_eq!(t.region.width, 640);
    }

    fn face(score: f32, x: f32, y: f32, w: f32, h: f32) -> Detection {
        let bbox = Rect::new(x, y, w, h).unwrap();
        Detection::new(ModelId::Yolo5FaceN, score, bbox, None, None).unwrap()
    }

    fn plate(score: f32, x: f32, y: f32, w: f32, h: f32) -> Detection {
        let bbox = Rect::new(x, y, w, h).unwrap();
        Detection::new(ModelId::PlateYolov9T, score, bbox, None, None).unwrap()
    }

    fn thresholds() -> Thresholds {
        let t = KindThresholds::new(0.5, 0.4).unwrap();
        Thresholds { face: t, plate: t }
    }

    fn finds(detections: Vec<Detection>) -> Found {
        Found {
            detections,
            ..Found::default()
        }
    }

    /// Two 1280-pixel tiles at half scale side by side on a 2240x640 image.
    fn halves() -> (Tile, Tile) {
        let tile = |x: u32, inner: [bool; 4]| Tile {
            region: Region {
                x,
                y: 0,
                width: 1280,
                height: 640,
            },
            input_width: 640,
            input_height: 320,
            inner,
        };
        (
            tile(0, [false, false, true, false]),
            tile(960, [true, false, false, false]),
        )
    }

    #[test]
    fn tile_findings_move_to_image_coordinates() {
        let (_, right) = halves();
        let lm = [Point { x: 10.0, y: 20.0 }; 5];
        let d = Detection::new(
            ModelId::Yolo5FaceN,
            0.9,
            Rect::new(100.0, 50.0, 20.0, 10.0).unwrap(),
            Some(lm),
            None,
        )
        .unwrap();
        let (moved, cut) = right.locate(&d);
        assert!(!cut);
        assert_eq!(moved.bbox(), Rect::new(1160.0, 100.0, 40.0, 20.0).unwrap());
        assert_eq!(moved.landmarks().unwrap()[0], Point { x: 980.0, y: 40.0 });
        assert_eq!((moved.score(), moved.model()), (0.9, ModelId::Yolo5FaceN));
    }

    #[test]
    fn a_box_on_an_inner_side_is_cut_and_one_on_the_border_is_not() {
        let (left, right) = halves();
        assert!(left.locate(&face(0.9, 600.0, 10.0, 40.0, 40.0)).1);
        assert!(!left.locate(&face(0.9, 0.0, 0.0, 40.0, 40.0)).1);
        assert!(!left.locate(&face(0.9, 100.0, 280.0, 40.0, 40.0)).1);
        assert!(right.locate(&face(0.9, 0.0, 10.0, 40.0, 40.0)).1);
        assert!(!right.locate(&face(0.9, 600.0, 10.0, 40.0, 40.0)).1);
    }

    #[test]
    fn a_cut_part_gives_way_to_the_whole_face_even_when_it_scores_higher() {
        let (left, right) = halves();
        // The face spans x 1240..1320 in the image: the left tile sees its left half at its
        // edge, the right tile all of it.
        let part = face(0.95, 620.0, 10.0, 20.0, 40.0);
        let whole_face = face(0.7, 140.0, 10.0, 40.0, 40.0);
        let merged = merge(
            Found::default(),
            vec![(left, finds(vec![part])), (right, finds(vec![whole_face]))],
            &thresholds(),
        );
        assert_eq!(merged.detections.len(), 1);
        assert_eq!(
            merged.detections[0].bbox(),
            Rect::new(1240.0, 20.0, 80.0, 80.0).unwrap()
        );
    }

    #[test]
    fn a_cut_part_nothing_else_covers_is_kept() {
        let (left, right) = halves();
        let part = face(0.6, 620.0, 10.0, 20.0, 40.0);
        let elsewhere = face(0.9, 400.0, 100.0, 40.0, 40.0);
        let a_plate_over_it = plate(0.9, 600.0, 0.0, 40.0, 60.0);
        let merged = merge(
            finds(Vec::new()),
            vec![
                (left, finds(vec![part, a_plate_over_it])),
                (right, finds(vec![elsewhere])),
            ],
            &thresholds(),
        );
        assert_eq!(merged.detections.len(), 3);
        assert!(merged.detections.iter().any(|d| d.bbox().x() == 1240.0));
    }

    #[test]
    fn the_same_face_in_several_views_is_kept_once_per_kind() {
        let (left, right) = halves();
        // In the overlap (x 960..1280): the left tile and the right tile both see it whole.
        let in_left = face(0.8, 520.0, 100.0, 20.0, 20.0);
        let in_right = face(0.9, 40.0, 100.0, 20.0, 20.0);
        let whole_view = face(0.6, 1040.0, 200.0, 41.0, 39.0);
        let a_plate_on_it = plate(0.7, 1040.0, 200.0, 40.0, 40.0);
        let merged = merge(
            finds(vec![whole_view, a_plate_on_it.clone()]),
            vec![(left, finds(vec![in_left])), (right, finds(vec![in_right]))],
            &thresholds(),
        );
        assert_eq!(merged.detections.len(), 2);
        assert_eq!(merged.detections[0].score(), 0.9);
        assert_eq!(merged.detections[1], a_plate_on_it);
    }

    #[test]
    fn boxes_larger_than_half_a_tile_are_left_to_the_coarser_views() {
        let (left, _) = halves();
        let large = |side: f32| plate(0.9, 10.0, 10.0, side, side * 0.5);
        let whole_view = plate(0.6, 1000.0, 100.0, 900.0, 500.0);
        let merged = merge(
            finds(vec![whole_view.clone()]),
            vec![(left, finds(vec![large(320.0), large(321.0)]))],
            &thresholds(),
        );
        let widths: Vec<f32> = merged.detections.iter().map(|d| d.bbox().width()).collect();
        assert_eq!(widths, [640.0, 900.0]);
    }

    #[test]
    fn faces_come_before_plates() {
        let (left, _) = halves();
        let merged = merge(
            finds(vec![face(0.6, 0.0, 0.0, 10.0, 10.0)]),
            vec![(left, finds(vec![plate(0.9, 100.0, 100.0, 10.0, 10.0)]))],
            &thresholds(),
        );
        let kinds: Vec<DetectionKind> = merged.detections.iter().map(|d| d.kind()).collect();
        assert_eq!(kinds, [DetectionKind::Face, DetectionKind::Plate]);
    }

    #[test]
    fn without_tiles_the_whole_view_stands_as_found() {
        // Out of score order and overlapping more than NMS allows: not touched again.
        let whole = Found {
            detections: vec![
                plate(0.9, 0.0, 0.0, 10.0, 10.0),
                face(0.6, 0.0, 0.0, 10.0, 10.0),
                face(0.7, 1.0, 0.0, 10.0, 10.0),
            ],
            over_limit: vec![(ModelId::YuNet, 1), (ModelId::YuNet, 2)],
            reran_on_cpu: true,
        };
        assert_eq!(merge(whole.clone(), Vec::new(), &thresholds()), whole);
    }

    #[test]
    fn dropped_counts_and_the_cpu_rerun_carry_over() {
        let (left, right) = halves();
        let whole = Found {
            over_limit: vec![(ModelId::YuNet, 2)],
            ..Found::default()
        };
        let capped = Found {
            over_limit: vec![(ModelId::YuNet, 3), (ModelId::PlateYolov9T, 1)],
            reran_on_cpu: true,
            ..Found::default()
        };
        let merged = merge(
            whole,
            vec![(left, capped), (right, Found::default())],
            &thresholds(),
        );
        assert_eq!(
            merged.over_limit,
            [(ModelId::YuNet, 5), (ModelId::PlateYolov9T, 1)]
        );
        assert!(merged.reran_on_cpu);
    }
}
