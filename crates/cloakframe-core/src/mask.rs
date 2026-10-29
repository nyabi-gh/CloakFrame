//! The mask renderer: fill, mosaic, blur and custom image over padded rectangles or ellipses,
//! with optional soft edges. The preview and the export call the same function (plan 4.3).
//!
//! The detected box is always fully replaced. Padding and soft edges only add to it.

use std::fmt;

use crate::detection::Detection;
use crate::geometry::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskError {
    /// The pixel buffer does not hold width x height x channels samples.
    BufferSize,
    PaddingOutOfRange,
    StrengthOutOfRange,
    /// The custom image method was chosen without an image.
    MissingOverlay,
}

impl fmt::Display for MaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::BufferSize => "pixel buffer does not match the image size",
            Self::PaddingOutOfRange => "padding is outside 0..=1",
            Self::StrengthOutOfRange => "mosaic strength is outside 4..=12",
            Self::MissingOverlay => "the custom image method needs an image",
        })
    }
}

impl std::error::Error for MaskError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Fill,
    Mosaic,
    Blur,
    CustomImage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Rectangle,
    Ellipse,
}

/// Mosaic cells along the short side of the masked area (D3). 1.12.1's pixel block size had no
/// effect on large faces because of its 12-cell cap; every strength here changes the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MosaicStrength(u8);

impl MosaicStrength {
    pub const MIN: u8 = 4;
    /// 1.12.1 capped the cells at 12; a finer mosaic than that hides less (review item 13).
    pub const MAX: u8 = 12;

    pub fn new(cells: u8) -> Result<Self, MaskError> {
        if !(Self::MIN..=Self::MAX).contains(&cells) {
            return Err(MaskError::StrengthOutOfRange);
        }
        Ok(Self(cells))
    }

    pub fn cells(self) -> u8 {
        self.0
    }
}

impl Default for MosaicStrength {
    fn default() -> Self {
        Self(8)
    }
}

/// Extra margin around each box as a fraction of its width and height on every side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Padding(f32);

impl Padding {
    pub fn new(ratio: f32) -> Result<Self, MaskError> {
        if !(0.0..=1.0).contains(&ratio) {
            return Err(MaskError::PaddingOutOfRange);
        }
        Ok(Self(ratio))
    }

    pub fn ratio(self) -> f32 {
        self.0
    }
}

impl Default for Padding {
    /// 1.12.1's default.
    fn default() -> Self {
        Self(0.18)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaskStyle {
    pub method: Method,
    pub shape: Shape,
    pub padding: Padding,
    pub soft_edges: bool,
    pub mosaic_strength: MosaicStrength,
}

impl Default for MaskStyle {
    /// A solid fill is the default (plan 5).
    fn default() -> Self {
        Self {
            method: Method::Fill,
            shape: Shape::Rectangle,
            padding: Padding::default(),
            soft_edges: false,
            mosaic_strength: MosaicStrength::default(),
        }
    }
}

/// One area to mask, in image pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    pub bbox: Rect,
    /// Face roll in radians; turns a custom image with the face.
    pub roll: Option<f32>,
    /// A mosaic's cells on this grid instead of fitted to the area (D3).
    pub grid: Option<MosaicGrid>,
}

impl Region {
    pub fn new(bbox: Rect) -> Self {
        Self {
            bbox,
            roll: None,
            grid: None,
        }
    }
}

impl From<&Detection> for Region {
    fn from(d: &Detection) -> Self {
        Self {
            roll: d.roll(),
            ..Self::new(d.bbox())
        }
    }
}

/// Square mosaic cells of one size whose edges are multiples of it from the image's origin, for
/// the frames of a video track (plan 4.4). A cell's value is the mean of the whole cell, so
/// where a still subject's box jitters from frame to frame its cells keep their values; cells
/// fitted to each frame's box gave those frames different means, and averaging the frames
/// recovered detail finer than the cells (docs/M5_RENDER.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MosaicGrid {
    cell: u32,
}

impl MosaicGrid {
    /// Cells are at least 2 pixels wide, as fitted cells are.
    pub const MIN_CELL: u32 = 2;

    pub fn new(cell: u32) -> Self {
        Self {
            cell: cell.max(Self::MIN_CELL),
        }
    }

    pub fn cell(self) -> u32 {
        self.cell
    }

    /// The cell that gives `bbox`'s padded area the strength's cells along its short side. A
    /// track takes the largest over its boxes, which never gives any of them more cells.
    pub fn cell_for(bbox: &Rect, style: &MaskStyle, width: u32, height: u32) -> u32 {
        if width == 0 || height == 0 {
            return Self::MIN_CELL;
        }
        let grown = grown_region(bbox, style.padding.ratio());
        let short = u32::try_from(grown.short()).unwrap_or(u32::MAX);
        short
            .div_ceil(u32::from(style.mosaic_strength.cells()))
            .max(Self::MIN_CELL)
    }
}

/// The user's replacement image, straight (not premultiplied) RGBA8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlay {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

impl Overlay {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, MaskError> {
        let (width, height) = (width as usize, height as usize);
        if width == 0 || height == 0 || rgba.len() != width * height * 4 {
            return Err(MaskError::BufferSize);
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    pub fn width(&self) -> u32 {
        self.width as u32
    }

    pub fn height(&self) -> u32 {
        self.height as u32
    }

    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Gray,
    GrayAlpha,
    Rgb,
    Rgba,
}

impl Layout {
    pub fn channels(self) -> usize {
        match self {
            Self::Gray => 1,
            Self::GrayAlpha => 2,
            Self::Rgb => 3,
            Self::Rgba => 4,
        }
    }

    fn has_alpha(self) -> bool {
        matches!(self, Self::GrayAlpha | Self::Rgba)
    }
}

#[derive(Debug)]
pub enum Pixels<'a> {
    U8(&'a mut [u8]),
    U16(&'a mut [u16]),
}

/// Interleaved pixels the renderer writes in place.
#[derive(Debug)]
pub struct ImageMut<'a> {
    width: usize,
    height: usize,
    layout: Layout,
    pixels: Pixels<'a>,
}

impl<'a> ImageMut<'a> {
    pub fn new(
        width: u32,
        height: u32,
        layout: Layout,
        pixels: Pixels<'a>,
    ) -> Result<Self, MaskError> {
        let (width, height) = (width as usize, height as usize);
        let len = match &pixels {
            Pixels::U8(p) => p.len(),
            Pixels::U16(p) => p.len(),
        };
        if len != width * height * layout.channels() {
            return Err(MaskError::BufferSize);
        }
        Ok(Self {
            width,
            height,
            layout,
            pixels,
        })
    }
}

/// Masks every region. Regions are applied in order; overlapping ones mask the already masked
/// pixels again.
pub fn render(
    image: &mut ImageMut<'_>,
    regions: &[Region],
    style: &MaskStyle,
    overlay: Option<&Overlay>,
) -> Result<(), MaskError> {
    if style.method == Method::CustomImage && overlay.is_none() {
        return Err(MaskError::MissingOverlay);
    }
    if image.width == 0 || image.height == 0 {
        return Ok(());
    }
    let (w, h, layout) = (image.width, image.height, image.layout);
    match &mut image.pixels {
        Pixels::U8(p) => {
            let mut plane = Plane::<u8>::borrow(p, w, h, layout);
            regions
                .iter()
                .for_each(|r| render_one(&mut plane, r, style, overlay));
        }
        Pixels::U16(p) => {
            let mut plane = Plane::<u16>::borrow(p, w, h, layout);
            regions
                .iter()
                .for_each(|r| render_one(&mut plane, r, style, overlay));
        }
    }
    Ok(())
}

trait Sample: Copy + Default {
    const MAX: u32;
    fn get(self) -> u32;
    /// Rounds to nearest, ties to even, and saturates (`cv::saturate_cast` from a double).
    fn from_f64(v: f64) -> Self;
    fn from_u32(v: u32) -> Self;
}

impl Sample for u8 {
    const MAX: u32 = 255;
    fn get(self) -> u32 {
        u32::from(self)
    }
    fn from_f64(v: f64) -> Self {
        v.round_ties_even().clamp(0.0, 255.0) as u8
    }
    fn from_u32(v: u32) -> Self {
        v.min(255) as u8
    }
}

impl Sample for u16 {
    const MAX: u32 = 65_535;
    fn get(self) -> u32 {
        u32::from(self)
    }
    fn from_f64(v: f64) -> Self {
        v.round_ties_even().clamp(0.0, 65_535.0) as u16
    }
    fn from_u32(v: u32) -> Self {
        v.min(65_535) as u16
    }
}

struct Plane<'a, S> {
    width: usize,
    height: usize,
    cn: usize,
    alpha: bool,
    data: &'a mut [S],
}

impl<'a, S: Sample> Plane<'a, S> {
    fn borrow(data: &'a mut [S], width: usize, height: usize, layout: Layout) -> Self {
        Self {
            width,
            height,
            cn: layout.channels(),
            alpha: layout.has_alpha(),
            data,
        }
    }

    fn at(&self, x: usize, y: usize) -> usize {
        (y * self.width + x) * self.cn
    }
}

/// Integer pixel rectangle, `x..x + w` by `y..y + h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IRect {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

impl IRect {
    fn right(&self) -> usize {
        self.x + self.w
    }

    fn bottom(&self) -> usize {
        self.y + self.h
    }

    fn contains(&self, x: usize, y: usize) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

/// The box grown by `padding` on every side and rounded outwards to whole pixels, before it is
/// clamped to the image. The strengths follow it: a face cut by the image's edge is masked as
/// strongly as a whole one (D3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Grown {
    x: i64,
    y: i64,
    w: u64,
    h: u64,
}

impl Grown {
    fn short(self) -> u64 {
        self.w.min(self.h)
    }
}

fn grown_region(bbox: &Rect, padding: f32) -> Grown {
    let pad_x = f64::from(bbox.width()) * f64::from(padding);
    let pad_y = f64::from(bbox.height()) * f64::from(padding);
    let left = (f64::from(bbox.x()) - pad_x).floor() as i64;
    let top = (f64::from(bbox.y()) - pad_y).floor() as i64;
    let right = (f64::from(bbox.right()) + pad_x).ceil() as i64;
    let bottom = (f64::from(bbox.bottom()) + pad_y).ceil() as i64;
    Grown {
        x: left,
        y: top,
        w: right.abs_diff(left).max(1),
        h: bottom.abs_diff(top).max(1),
    }
}

/// The box grown by `padding` on every side, rounded outwards to whole pixels and clamped to the
/// image, never empty (1.12.1 `paddedRegion`).
fn padded_region(bbox: &Rect, padding: f32, width: usize, height: usize) -> IRect {
    let (w, h) = (width as f64, height as f64);
    let pad_x = f64::from(bbox.width()) * f64::from(padding);
    let pad_y = f64::from(bbox.height()) * f64::from(padding);
    let left = (f64::from(bbox.x()) - pad_x).clamp(0.0, w).floor() as usize;
    let top = (f64::from(bbox.y()) - pad_y).clamp(0.0, h).floor() as usize;
    let right = (f64::from(bbox.right()) + pad_x).clamp(0.0, w).ceil() as usize;
    let bottom = (f64::from(bbox.bottom()) + pad_y).clamp(0.0, h).ceil() as usize;
    let x = left.min(width - 1);
    let y = top.min(height - 1);
    let right = right.clamp(x + 1, width);
    let bottom = bottom.clamp(y + 1, height);
    IRect {
        x,
        y,
        w: right - x,
        h: bottom - y,
    }
}

const INNER_TRANSITION_RATIO: f32 = 0.12;
const OUTER_TRANSITION_RATIO: f32 = 0.10;
const MIN_OUTER_TRANSITION: usize = 4;

fn render_one<S: Sample>(
    plane: &mut Plane<'_, S>,
    region: &Region,
    style: &MaskStyle,
    overlay: Option<&Overlay>,
) {
    if region.bbox.width() <= 0.0 || region.bbox.height() <= 0.0 {
        return;
    }
    let (width, height) = (plane.width, plane.height);
    let padded = padded_region(&region.bbox, style.padding.ratio(), width, height);
    let grown = grown_region(&region.bbox, style.padding.ratio());
    let detected = padded_region(&region.bbox, 0.0, width, height);

    if style.method == Method::CustomImage {
        let overlay = overlay.expect("checked by render");
        let mut area = Effect::copy(plane, padded);
        mosaic(
            plane,
            &mut area,
            (padded, grown),
            style.mosaic_strength,
            region.grid,
        );
        custom_image(&mut area, overlay, region.roll);
        area.write_back(plane, |_, _| 1.0);
        return;
    }

    if !style.soft_edges {
        let mut area = Effect::copy(plane, padded);
        apply_effect(plane, &mut area, (padded, grown), style, region.grid);
        match style.shape {
            Shape::Rectangle => area.write_back(plane, |_, _| 1.0),
            Shape::Ellipse => {
                let ellipse = EllipseMask::new(padded);
                area.write_back(plane, |x, y| {
                    f32::from(u8::from(ellipse.covers(x, y) || detected.contains(x, y)))
                })
            }
        }
        return;
    }

    let short = padded.w.min(padded.h) as f32;
    let inset = (detected.x - padded.x)
        .min(detected.y - padded.y)
        .min(padded.right() - detected.right())
        .min(padded.bottom() - detected.bottom());
    let inner = inset.min((short * INNER_TRANSITION_RATIO).round() as usize);
    let outer = MIN_OUTER_TRANSITION.max((short * OUTER_TRANSITION_RATIO).round() as usize);
    let left = padded.x.saturating_sub(outer);
    let top = padded.y.saturating_sub(outer);
    let outer_rect = IRect {
        x: left,
        y: top,
        w: (padded.right() + outer).min(width) - left,
        h: (padded.bottom() + outer).min(height) - top,
    };
    let mut area = Effect::copy(plane, outer_rect);
    apply_effect(plane, &mut area, (padded, grown), style, region.grid);
    let ramp = SoftEdge {
        core: padded,
        shape: style.shape,
        inner: inner as f32,
        outer: outer as f32,
    };
    area.write_back(plane, |x, y| {
        if style.shape == Shape::Ellipse && detected.contains(x, y) {
            1.0
        } else {
            ramp.weight(x, y)
        }
    });
}

fn apply_effect<S: Sample>(
    plane: &Plane<'_, S>,
    area: &mut Effect<S>,
    (padded, grown): (IRect, Grown),
    style: &MaskStyle,
    grid: Option<MosaicGrid>,
) {
    match style.method {
        Method::Fill => fill(area),
        Method::Mosaic => mosaic(plane, area, (padded, grown), style.mosaic_strength, grid),
        Method::Blur => blur(plane, area, blur_sigma(grown, plane.width, plane.height)),
        Method::CustomImage => unreachable!("handled before"),
    }
}

/// A copy of one rectangle of the image that an effect rewrites before it is blended back.
struct Effect<S> {
    rect: IRect,
    cn: usize,
    alpha: bool,
    data: Vec<S>,
}

impl<S: Sample> Effect<S> {
    fn copy(plane: &Plane<'_, S>, rect: IRect) -> Self {
        let cn = plane.cn;
        let mut data = Vec::with_capacity(rect.w * rect.h * cn);
        for y in rect.y..rect.bottom() {
            let start = plane.at(rect.x, y);
            data.extend_from_slice(&plane.data[start..start + rect.w * cn]);
        }
        Self {
            rect,
            cn,
            alpha: plane.alpha,
            data,
        }
    }

    fn at(&self, x: usize, y: usize) -> usize {
        ((y - self.rect.y) * self.rect.w + (x - self.rect.x)) * self.cn
    }

    /// Blends into the image with `weight(x, y)` in 0..=1 per image pixel, as
    /// `original + (changed - original) * weight` rounded like 1.12.1's `blendWithMask`.
    fn write_back(&self, plane: &mut Plane<'_, S>, weight: impl Fn(usize, usize) -> f32) {
        for y in self.rect.y..self.rect.bottom() {
            for x in self.rect.x..self.rect.right() {
                let w = weight(x, y);
                if w <= 0.0 {
                    continue;
                }
                let (src, dst) = (self.at(x, y), plane.at(x, y));
                for c in 0..self.cn {
                    let changed = self.data[src + c];
                    plane.data[dst + c] = if w >= 1.0 {
                        changed
                    } else {
                        let original = f64::from(plane.data[dst + c].get());
                        let v = original + (f64::from(changed.get()) - original) * f64::from(w);
                        S::from_f64(v)
                    };
                }
            }
        }
    }
}

/// Black, and opaque where there is alpha: a fill must not leave a see-through hole.
fn fill<S: Sample>(area: &mut Effect<S>) {
    let cn = area.cn;
    for px in area.data.chunks_exact_mut(cn) {
        px.fill(S::default());
        if area.alpha {
            px[cn - 1] = S::from_u32(S::MAX);
        }
    }
}

/// Cell boundaries along one side of `cells` cells over `len` pixels from `start`, at
/// `start + floor(i * len / cells)`, of the cells that reach into `clip` (start, length), as
/// offsets into it and cut to it.
fn cell_edges(start: i64, len: u64, cells: u64, clip: (usize, usize)) -> Vec<usize> {
    let edge = |i: u64| start + (u128::from(i) * u128::from(len) / u128::from(cells)) as i64;
    let (lo, hi) = (clip.0 as i64, (clip.0 + clip.1) as i64);
    // The first cell ending after `lo`, from an estimate that rounding can leave one off.
    let estimate = u128::from(lo.saturating_sub(start).max(0).unsigned_abs()) * u128::from(cells)
        / u128::from(len);
    let mut first = u64::try_from(estimate).unwrap_or(cells).min(cells - 1);
    while first > 0 && edge(first) > lo {
        first -= 1;
    }
    while first + 1 < cells && edge(first + 1) <= lo {
        first += 1;
    }
    let mut edges = vec![0];
    let mut i = first + 1;
    while i < cells && edge(i) < hi {
        edges.push((edge(i) - lo) as usize);
        i += 1;
    }
    edges.push(clip.1);
    edges
}

/// Mosaic over `padded`, on `grid` if given and otherwise fitted to `grown`, the region before it
/// was clamped to the image: cells along the short side are the strength, but at least 2 pixels
/// wide so a small region still changes; the long side gets cells of about the same size. Each
/// cell is the rounded mean of its pixels in the image. Pixels of `area` outside `padded` (the
/// soft-edge band) take the nearest edge cell of a fitted mosaic and their own cell on a grid.
fn mosaic<S: Sample>(
    plane: &Plane<'_, S>,
    area: &mut Effect<S>,
    (padded, grown): (IRect, Grown),
    strength: MosaicStrength,
    grid: Option<MosaicGrid>,
) {
    if let Some(grid) = grid {
        return grid_mosaic(plane, area, grid.cell as usize);
    }
    let (short, long) = (grown.short(), grown.w.max(grown.h));
    let cells_short = u64::from(strength.cells()).min((short / 2).max(1));
    let cells_long = (cells_short as f64 * long as f64 / short as f64).round() as u64;
    let cells_long = cells_long.clamp(1, (long / 2).max(1));
    let (cells_x, cells_y) = if grown.w <= grown.h {
        (cells_short, cells_long)
    } else {
        (cells_long, cells_short)
    };
    let xs = cell_edges(grown.x, grown.w, cells_x, (padded.x, padded.w));
    let ys = cell_edges(grown.y, grown.h, cells_y, (padded.y, padded.h));
    let (cx, cy) = (xs.len() - 1, ys.len() - 1);
    let cn = area.cn;
    let mut means = vec![S::default(); cx * cy * cn];
    for j in 0..cy {
        for i in 0..cx {
            let mut sums = [0u64; 4];
            for y in ys[j]..ys[j + 1] {
                for x in xs[i]..xs[i + 1] {
                    let at = area.at(padded.x + x, padded.y + y);
                    for (c, sum) in sums.iter_mut().enumerate().take(cn) {
                        *sum += u64::from(area.data[at + c].get());
                    }
                }
            }
            let n = ((xs[i + 1] - xs[i]) * (ys[j + 1] - ys[j])) as u64;
            for c in 0..cn {
                means[(j * cx + i) * cn + c] = S::from_u32(((sums[c] + n / 2) / n) as u32);
            }
        }
    }
    let cell_of = |edges: &[usize], v: isize| {
        let v = v.clamp(0, *edges.last().unwrap() as isize - 1) as usize;
        edges.partition_point(|&e| e <= v) - 1
    };
    let rect = area.rect;
    for y in rect.y..rect.bottom() {
        let j = cell_of(&ys, y as isize - padded.y as isize);
        for x in rect.x..rect.right() {
            let i = cell_of(&xs, x as isize - padded.x as isize);
            let at = area.at(x, y);
            let mean = &means[(j * cx + i) * cn..(j * cx + i + 1) * cn];
            area.data[at..at + cn].copy_from_slice(mean);
        }
    }
}

/// Every pixel of `area` takes the rounded mean of its grid cell, the whole cell as the image has
/// it (clipped to the image), not only the part inside the area.
fn grid_mosaic<S: Sample>(plane: &Plane<'_, S>, area: &mut Effect<S>, cell: usize) {
    let cn = area.cn;
    let rect = area.rect;
    let (first_i, first_j) = (rect.x / cell, rect.y / cell);
    let (cx, cy) = (
        (rect.right() - 1) / cell + 1 - first_i,
        (rect.bottom() - 1) / cell + 1 - first_j,
    );
    let mut means = vec![S::default(); cx * cy * cn];
    for j in 0..cy {
        let (y0, y1) = (
            (first_j + j) * cell,
            ((first_j + j + 1) * cell).min(plane.height),
        );
        for i in 0..cx {
            let (x0, x1) = (
                (first_i + i) * cell,
                ((first_i + i + 1) * cell).min(plane.width),
            );
            let mut sums = [0u64; 4];
            for y in y0..y1 {
                let row = plane.at(x0, y);
                for px in plane.data[row..row + (x1 - x0) * cn].chunks_exact(cn) {
                    for (sum, v) in sums.iter_mut().zip(px) {
                        *sum += u64::from(v.get());
                    }
                }
            }
            let n = ((x1 - x0) * (y1 - y0)) as u64;
            for c in 0..cn {
                means[(j * cx + i) * cn + c] = S::from_u32(((sums[c] + n / 2) / n) as u32);
            }
        }
    }
    for y in rect.y..rect.bottom() {
        let j = y / cell - first_j;
        for x in rect.x..rect.right() {
            let i = x / cell - first_i;
            let at = area.at(x, y);
            area.data[at..at + cn]
                .copy_from_slice(&means[(j * cx + i) * cn..(j * cx + i + 1) * cn]);
        }
    }
}

/// The weakest blur: below this a thin or small region keeps readable detail (plan 5).
const MIN_BLUR_SIGMA: f32 = 4.0;

/// The region's short side / 6 (1.12.1), at least [`MIN_BLUR_SIGMA`]. A region larger than the
/// image counts as the image's longer side: each box pass starts with a sum over its whole
/// kernel.
fn blur_sigma(grown: Grown, width: usize, height: usize) -> f32 {
    let short = grown.short().min(width.max(height) as u64);
    (short as f32 / 6.0).max(MIN_BLUR_SIGMA)
}

/// Gaussian blur. Pixels around the area feed the kernel too, so its edges are as blurred as its
/// middle (plan 5: the kernel is not cut at the region). Three box passes approximate the
/// Gaussian in time independent of sigma; image borders repeat the edge pixel.
fn blur<S: Sample>(plane: &Plane<'_, S>, area: &mut Effect<S>, sigma: f32) {
    let boxes = box_radii(sigma);
    let reach: usize = boxes.iter().sum();
    let rect = area.rect;
    let left = rect.x.saturating_sub(reach);
    let top = rect.y.saturating_sub(reach);
    let window = IRect {
        x: left,
        y: top,
        w: (rect.right() + reach).min(plane.width) - left,
        h: (rect.bottom() + reach).min(plane.height) - top,
    };
    let cn = plane.cn;
    let mut values: Vec<f32> = Vec::with_capacity(window.w * window.h * cn);
    for y in window.y..window.bottom() {
        let start = plane.at(window.x, y);
        values.extend(
            plane.data[start..start + window.w * cn]
                .iter()
                .map(|s| s.get() as f32),
        );
    }
    let mut scratch = vec![0f32; window.w.max(window.h)];
    for &r in &boxes {
        for y in 0..window.h {
            for c in 0..cn {
                let at = |x: usize| (y * window.w + x) * cn + c;
                box_pass(&mut values, &mut scratch, window.w, r, at);
            }
        }
        for x in 0..window.w {
            for c in 0..cn {
                let at = |y: usize| (y * window.w + x) * cn + c;
                box_pass(&mut values, &mut scratch, window.h, r, at);
            }
        }
    }
    for y in rect.y..rect.bottom() {
        for x in rect.x..rect.right() {
            let src = ((y - window.y) * window.w + (x - window.x)) * cn;
            let dst = area.at(x, y);
            for c in 0..cn {
                area.data[dst + c] = S::from_f64(f64::from(values[src + c]));
            }
        }
    }
}

/// Radii of three box filters whose repeated application has standard deviation `sigma`.
fn box_radii(sigma: f32) -> [usize; 3] {
    let n = 3.0f32;
    let ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut lower = ideal.floor() as i32;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let lf = lower as f32;
    let m = ((12.0 * sigma * sigma - n * lf * lf - 4.0 * n * lf - 3.0 * n) / (-4.0 * lf - 4.0))
        .round() as i32;
    std::array::from_fn(|i| {
        let width = if (i as i32) < m { lower } else { lower + 2 };
        (width.max(1) as usize - 1) / 2
    })
}

/// Box filter of radius `r` along one line of `len` samples at `at(i)`, repeating the end
/// samples past the line.
fn box_pass(
    values: &mut [f32],
    scratch: &mut [f32],
    len: usize,
    r: usize,
    at: impl Fn(usize) -> usize,
) {
    if r == 0 {
        return;
    }
    let get = |values: &[f32], i: isize| values[at(i.clamp(0, len as isize - 1) as usize)];
    let r = r as isize;
    let mut sum: f32 = (-r..=r).map(|i| get(values, i)).sum();
    let norm = 1.0 / (2 * r + 1) as f32;
    for i in 0..len as isize {
        scratch[i as usize] = sum * norm;
        sum += get(values, i + r + 1) - get(values, i - r);
    }
    for (i, &v) in scratch[..len].iter().enumerate() {
        values[at(i)] = v;
    }
}

/// The ellipse inscribed in a rectangle, grown by one pixel; a pixel is masked when that ellipse
/// touches any part of it. 1.12.1 masked every pixel its antialiased ellipse touched at all; this
/// covers at least that except for single boundary pixels (0.03% of them over 1–200 px sizes).
struct EllipseMask {
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
}

impl EllipseMask {
    fn new(r: IRect) -> Self {
        Self {
            cx: (r.x + r.w / 2) as f64,
            cy: (r.y + r.h / 2) as f64,
            rx: (r.w / 2) as f64 + 1.0,
            ry: (r.h / 2) as f64 + 1.0,
        }
    }

    fn covers(&self, x: usize, y: usize) -> bool {
        let (x, y) = (x as f64, y as f64);
        let dx = (self.cx.clamp(x - 0.5, x + 0.5) - self.cx) / self.rx;
        let dy = (self.cy.clamp(y - 0.5, y + 0.5) - self.cy) / self.ry;
        dx * dx + dy * dy <= 1.0
    }
}

/// Smoothstep weight from 0 at `outer` pixels outside `core` to 1 at `inner` pixels inside it
/// (1.12.1 `softTransitionMask`). The rectangle's distance is exact, as 1.12.1's distance
/// transform was; the ellipse uses the radial distance of 1.12.1's large-image path.
struct SoftEdge {
    core: IRect,
    shape: Shape,
    inner: f32,
    outer: f32,
}

impl SoftEdge {
    fn weight(&self, x: usize, y: usize) -> f32 {
        let (x, y) = (x as f32, y as f32);
        let c = &self.core;
        let (cx0, cy0) = (c.x as f32, c.y as f32);
        let (cw, ch) = (c.w as f32, c.h as f32);
        let signed = match self.shape {
            Shape::Ellipse => {
                // The centre and axes 1.12.1 drew the ellipse with (whole pixels).
                let (hw, hh) = ((c.w / 2) as f32, (c.h / 2) as f32);
                let (rx, ry) = (hw.max(0.5), hh.max(0.5));
                let dx = (x - (cx0 + hw)) / rx;
                let dy = (y - (cy0 + hh)) / ry;
                // Grown by a pixel like the hard ellipse, whose rasterized edge 1.12.1 counted
                // as inside.
                (1.0 - (dx * dx + dy * dy).sqrt()) * rx.min(ry) + 1.0
            }
            Shape::Rectangle => {
                let out_x = (cx0 - x).max(0.0).max(x - (cx0 + cw - 1.0));
                let out_y = (cy0 - y).max(0.0).max(y - (cy0 + ch - 1.0));
                if out_x > 0.0 || out_y > 0.0 {
                    -out_x.hypot(out_y)
                } else {
                    (x - cx0 + 1.0)
                        .min(y - cy0 + 1.0)
                        .min(cx0 + cw - x)
                        .min(cy0 + ch - y)
                }
            }
        };
        let t = ((signed + self.outer) / (self.inner + self.outer)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
}

/// Draws the overlay over the (already mosaicked) area: cropped to the area's aspect, turned by
/// the face roll and alpha-blended (1.12.1 `customImageRegion`).
fn custom_image<S: Sample>(area: &mut Effect<S>, overlay: &Overlay, roll: Option<f32>) {
    let (tw, th) = (area.rect.w, area.rect.h);
    let rendered = overlay_pixels(overlay, tw, th, roll);
    let cn = area.cn;
    let scale = f64::from(S::MAX) / 255.0;
    for (i, px) in area.data.chunks_exact_mut(cn).enumerate() {
        let [r, g, b, a] = rendered[i];
        if a <= 0.0 {
            continue;
        }
        let weight = f64::from(a / 255.0);
        let color: [f32; 3] = [r, g, b];
        let colors = if cn <= 2 {
            // cv::COLOR_BGRA2GRAY weights.
            let gray = 0.299 * color[0] + 0.587 * color[1] + 0.114 * color[2];
            [gray; 3]
        } else {
            color
        };
        let color_channels = if area.alpha { cn - 1 } else { cn };
        for (c, v) in px.iter_mut().take(color_channels).enumerate() {
            let changed = f64::from(colors[c].round().clamp(0.0, 255.0)) * scale;
            let original = f64::from(v.get());
            *v = S::from_f64(original + (changed - original) * weight);
        }
    }
}

/// The overlay as straight RGBA in 0..=255 for each pixel of a `tw` x `th` target.
fn overlay_pixels(overlay: &Overlay, tw: usize, th: usize, roll: Option<f32>) -> Vec<[f32; 4]> {
    let premultiplied: Vec<[f32; 4]> = overlay
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let a = f32::from(p[3]);
            [
                f32::from(p[0]) * a / 255.0,
                f32::from(p[1]) * a / 255.0,
                f32::from(p[2]) * a / 255.0,
                a,
            ]
        })
        .collect();
    let roll = roll.filter(|r| r.abs() > 0.001);
    let (cos, sin) = roll.map_or((1.0, 0.0), |r| (r.cos().abs(), r.sin().abs()));
    let overscan = if roll.is_some() { 2 } else { 0 };
    let (twf, thf) = (tw as f32, th as f32);
    let canvas_w = tw.max((cos * twf + sin * thf).ceil() as usize + overscan);
    let canvas_h = th.max((sin * twf + cos * thf).ceil() as usize + overscan);

    let (ow, oh) = (overlay.width, overlay.height);
    let source_aspect = ow as f64 / oh as f64;
    let canvas_aspect = canvas_w as f64 / canvas_h as f64;
    let (mut cx, mut cy, mut cw, mut ch) = (0, 0, ow, oh);
    if source_aspect > canvas_aspect {
        cw = ((oh as f64 * canvas_aspect).round() as usize).clamp(1, ow);
        cx = (ow - cw) / 2;
    } else if source_aspect < canvas_aspect {
        ch = ((ow as f64 / canvas_aspect).round() as usize).clamp(1, oh);
        cy = (oh - ch) / 2;
    }
    let crop: Vec<[f32; 4]> = (cy..cy + ch)
        .flat_map(|y| premultiplied[y * ow + cx..y * ow + cx + cw].iter().copied())
        .collect();
    let canvas = resample(&crop, cw, ch, canvas_w, canvas_h);

    let mut out = match roll {
        None => canvas,
        Some(r) => {
            let (sc_x, sc_y) = ((canvas_w as f32 - 1.0) * 0.5, (canvas_h as f32 - 1.0) * 0.5);
            let (tc_x, tc_y) = ((twf - 1.0) * 0.5, (thf - 1.0) * 0.5);
            let (c, s) = (r.cos(), r.sin());
            let mut out = Vec::with_capacity(tw * th);
            for y in 0..th {
                for x in 0..tw {
                    let (dx, dy) = (x as f32 - tc_x, y as f32 - tc_y);
                    // Inverse of turning the canvas by `roll` about its centre.
                    let sx = c * dx + s * dy + sc_x;
                    let sy = -s * dx + c * dy + sc_y;
                    out.push(bilinear(&canvas, canvas_w, canvas_h, sx, sy));
                }
            }
            out
        }
    };
    for p in &mut out {
        let alpha = p[3];
        if alpha > 0.0 {
            for v in &mut p[..3] {
                *v = (*v * 255.0 / alpha).min(255.0);
            }
        } else {
            *p = [0.0; 4];
        }
    }
    out
}

/// Bilinear sample at pixel-centre coordinates; transparent outside the image.
fn bilinear(src: &[[f32; 4]], w: usize, h: usize, x: f32, y: f32) -> [f32; 4] {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let texel = |xi: f32, yi: f32| {
        if xi < 0.0 || yi < 0.0 || xi >= w as f32 || yi >= h as f32 {
            [0.0; 4]
        } else {
            src[yi as usize * w + xi as usize]
        }
    };
    let (a, b) = (texel(x0, y0), texel(x0 + 1.0, y0));
    let (c, d) = (texel(x0, y0 + 1.0), texel(x0 + 1.0, y0 + 1.0));
    std::array::from_fn(|i| {
        let top = a[i] + (b[i] - a[i]) * fx;
        let bottom = c[i] + (d[i] - c[i]) * fx;
        top + (bottom - top) * fy
    })
}

/// Resizes premultiplied pixels: area averaging along an axis that shrinks, linear along one
/// that grows (half-pixel centres, edges repeated).
fn resample(src: &[[f32; 4]], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<[f32; 4]> {
    let taps_x = taps(sw, dw);
    let taps_y = taps(sh, dh);
    let mut rows = vec![[0f32; 4]; dw * sh];
    for y in 0..sh {
        for (x, t) in taps_x.iter().enumerate() {
            rows[y * dw + x] = weighted(t, |i| src[y * sw + i]);
        }
    }
    let mut out = vec![[0f32; 4]; dw * dh];
    for (y, t) in taps_y.iter().enumerate() {
        for x in 0..dw {
            out[y * dw + x] = weighted(t, |i| rows[i * dw + x]);
        }
    }
    out
}

fn weighted(taps: &[(usize, f32)], get: impl Fn(usize) -> [f32; 4]) -> [f32; 4] {
    let mut acc = [0f32; 4];
    for &(i, w) in taps {
        let v = get(i);
        for c in 0..4 {
            acc[c] += v[c] * w;
        }
    }
    acc
}

/// Source indices and weights for each of `dst` samples covering `src` samples.
fn taps(src: usize, dst: usize) -> Vec<Vec<(usize, f32)>> {
    let scale = src as f64 / dst as f64;
    (0..dst)
        .map(|d| {
            if scale > 1.0 {
                let (start, end) = (d as f64 * scale, (d + 1) as f64 * scale);
                let mut taps = Vec::new();
                let mut i = start.floor() as usize;
                while (i as f64) < end && i < src {
                    let overlap = end.min((i + 1) as f64) - start.max(i as f64);
                    taps.push((i, (overlap / scale) as f32));
                    i += 1;
                }
                taps
            } else {
                let pos = ((d as f64 + 0.5) * scale - 0.5).max(0.0);
                let i = (pos.floor() as usize).min(src - 1);
                let f = (pos - i as f64) as f32;
                if i + 1 < src && f > 0.0 {
                    vec![(i, 1.0 - f), (i + 1, f)]
                } else {
                    vec![(i, 1.0)]
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(x, y, w, h).unwrap()
    }

    fn region(x: f32, y: f32, w: f32, h: f32) -> Region {
        Region::new(rect(x, y, w, h))
    }

    fn style(method: Method, shape: Shape, padding: f32, soft_edges: bool) -> MaskStyle {
        MaskStyle {
            method,
            shape,
            padding: Padding::new(padding).unwrap(),
            soft_edges,
            ..MaskStyle::default()
        }
    }

    #[test]
    fn padded_regions_round_outwards_and_stay_in_the_image() {
        let r = padded_region(&rect(20.0, 20.0, 24.0, 24.0), 0.18, 64, 64);
        assert_eq!(
            r,
            IRect {
                x: 15,
                y: 15,
                w: 34,
                h: 34
            }
        );
        let edge = padded_region(&rect(-5.0, 60.5, 10.0, 10.0), 0.0, 64, 64);
        assert_eq!(
            edge,
            IRect {
                x: 0,
                y: 60,
                w: 5,
                h: 4
            }
        );
        let outside = padded_region(&rect(70.0, 70.0, 5.0, 5.0), 0.0, 64, 64);
        assert_eq!(
            outside,
            IRect {
                x: 63,
                y: 63,
                w: 1,
                h: 1
            }
        );
    }

    /// A face cut by the image's edge gets the cells and the blur of the whole face (D3), not
    /// of the part left in the image.
    #[test]
    fn a_face_cut_by_the_edge_is_masked_as_strongly_as_a_whole_one() {
        let runs = |bbox: Rect| {
            let (w, h) = (200u32, 96u32);
            let mut px: Vec<u8> = (0..h).flat_map(|_| 0..w as u8).collect();
            let mut image = ImageMut::new(w, h, Layout::Gray, Pixels::U8(&mut px)).unwrap();
            let s = style(Method::Mosaic, Shape::Rectangle, 0.0, false);
            render(&mut image, &[Region::new(bbox)], &s, None).unwrap();
            let row = &px[48 * w as usize..49 * w as usize];
            let (from, to) = (bbox.x().max(0.0) as usize, bbox.right() as usize);
            1 + row[from..to].windows(2).filter(|p| p[0] != p[1]).count()
        };
        assert_eq!(
            runs(rect(100.0, 0.0, 96.0, 96.0)),
            8,
            "the strength's 8 cells"
        );
        assert_eq!(runs(rect(-48.0, 0.0, 96.0, 96.0)), 4, "half of them");

        let s = MaskStyle {
            method: Method::Mosaic,
            ..MaskStyle::default()
        };
        assert_eq!(
            MosaicGrid::cell_for(&rect(-200.0, 100.0, 300.0, 360.0), &s, 1920, 1080),
            MosaicGrid::cell_for(&rect(800.0, 100.0, 300.0, 360.0), &s, 1920, 1080),
        );

        let sigma = |bbox: Rect| blur_sigma(grown_region(&bbox, 0.18), 1920, 1080);
        assert_eq!(
            sigma(rect(-200.0, 100.0, 300.0, 360.0)),
            sigma(rect(800.0, 100.0, 300.0, 360.0))
        );
        assert_eq!(
            sigma(rect(-5000.0, -5000.0, 20_000.0, 20_000.0)),
            1920.0 / 6.0,
            "no more than a box the size of the image"
        );
    }

    /// Plan 9.1: no shape, padding or soft edge leaves any pixel of the detected box unmasked,
    /// on 8-bit RGB and 16-bit RGBA, including boxes past the image border.
    #[test]
    fn the_detected_box_is_always_fully_masked() {
        let mut seed = 12_345u32;
        let mut next = |n: u32| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (seed >> 8) % n
        };
        for _ in 0..300 {
            let (w, h) = (8 + next(90), 8 + next(90));
            let bbox = rect(
                next(w + 10) as f32 - 5.0 + next(4) as f32 * 0.25,
                next(h + 10) as f32 - 5.0 + next(4) as f32 * 0.25,
                1.0 + next(w) as f32 + next(4) as f32 * 0.25,
                1.0 + next(h) as f32 + next(4) as f32 * 0.25,
            );
            let shape = if next(2) == 0 {
                Shape::Rectangle
            } else {
                Shape::Ellipse
            };
            let s = style(Method::Fill, shape, next(5) as f32 * 0.25, next(2) == 0);
            let detected = padded_region(&bbox, 0.0, w as usize, h as usize);
            let grid = (next(2) == 0).then(|| MosaicGrid::new(next(12)));
            let r = Region {
                grid,
                ..Region::new(bbox)
            };

            let mut rgb = vec![200u8; (w * h * 3) as usize];
            let mut image = ImageMut::new(w, h, Layout::Rgb, Pixels::U8(&mut rgb)).unwrap();
            render(&mut image, &[r], &s, None).unwrap();
            let mut rgba = vec![40_000u16; (w * h * 4) as usize];
            let mut deep = ImageMut::new(w, h, Layout::Rgba, Pixels::U16(&mut rgba)).unwrap();
            render(&mut deep, &[r], &s, None).unwrap();

            for y in detected.y..detected.bottom() {
                for x in detected.x..detected.right() {
                    let at = y * w as usize + x;
                    assert_eq!(rgb[at * 3..at * 3 + 3], [0, 0, 0], "{bbox:?} {s:?}");
                    assert_eq!(
                        rgba[at * 4..at * 4 + 4],
                        [0, 0, 0, 65_535],
                        "{bbox:?} {s:?}"
                    );
                }
            }

            // The other methods take the same weights: inside the box each one gives exactly
            // what its hard rectangle over the same padded region gives.
            let overlay = Overlay::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
            let source: Vec<u8> = (0..w * h * 3).map(|i| (i * 7 % 251) as u8).collect();
            for method in [Method::Mosaic, Method::Blur, Method::CustomImage] {
                let shaped = style(method, s.shape, s.padding.ratio(), s.soft_edges);
                let hard = style(method, Shape::Rectangle, s.padding.ratio(), false);
                let mut got = source.clone();
                let mut want = source.clone();
                for (px, st) in [(&mut got, &shaped), (&mut want, &hard)] {
                    let mut image = ImageMut::new(w, h, Layout::Rgb, Pixels::U8(px)).unwrap();
                    render(&mut image, &[r], st, Some(&overlay)).unwrap();
                }
                // Blur sums each line in f32 from its window's start, and a soft edge widens the
                // window, so a blurred value can round one level apart. A weight below 1 would
                // move a pixel towards its source value, which is far from the blur here.
                let tolerance = if method == Method::Blur { 1 } else { 0 };
                for y in detected.y..detected.bottom() {
                    let row =
                        (y * w as usize + detected.x) * 3..(y * w as usize + detected.right()) * 3;
                    let diff = got[row.clone()]
                        .iter()
                        .zip(&want[row])
                        .map(|(a, b)| a.abs_diff(*b))
                        .max()
                        .unwrap_or(0);
                    assert!(diff <= tolerance, "{bbox:?} {shaped:?}: {diff}");
                }
            }
        }
    }

    /// A still subject whose box jitters keeps its cells' values on a grid: every pixel two
    /// frames both mask is the same. Fitted cells move with the box (docs/M5_RENDER.md).
    #[test]
    fn a_grid_gives_a_jittering_box_the_same_cells() {
        let (w, h) = (160u32, 120u32);
        let source: Vec<u8> = (0..w * h * 3).map(|i| (i * 37 % 251) as u8).collect();
        let s = style(Method::Mosaic, Shape::Rectangle, 0.18, false);
        let boxes = [
            rect(50.0, 30.0, 40.0, 50.0),
            rect(51.5, 29.0, 38.0, 52.5),
            rect(48.0, 31.25, 43.0, 47.0),
        ];
        let cell = boxes
            .iter()
            .map(|b| MosaicGrid::cell_for(b, &s, w, h))
            .max()
            .unwrap();
        let masked = |bbox: Rect, grid: Option<MosaicGrid>| {
            let mut px = source.clone();
            let mut image = ImageMut::new(w, h, Layout::Rgb, Pixels::U8(&mut px)).unwrap();
            let region = Region {
                grid,
                ..Region::new(bbox)
            };
            render(&mut image, &[region], &s, None).unwrap();
            px
        };
        let both = |a: &[u8], b: &[u8], same: bool| {
            let mut shared = 0;
            for i in (0..a.len()).step_by(3) {
                let masked_in_both =
                    a[i..i + 3] != source[i..i + 3] && b[i..i + 3] != source[i..i + 3];
                if masked_in_both {
                    shared += 1;
                    if same {
                        assert_eq!(a[i..i + 3], b[i..i + 3], "pixel {}", i / 3);
                    }
                }
            }
            assert!(shared > 1000);
            (0..a.len())
                .step_by(3)
                .filter(|&i| a[i..i + 3] != source[i..i + 3] && b[i..i + 3] != source[i..i + 3])
                .any(|i| a[i..i + 3] != b[i..i + 3])
        };
        let grid = Some(MosaicGrid::new(cell));
        let first = masked(boxes[0], grid);
        for &b in &boxes[1..] {
            both(&first, &masked(b, grid), true);
        }
        let fitted = masked(boxes[0], None);
        assert!(
            boxes[1..]
                .iter()
                .any(|&b| both(&fitted, &masked(b, None), false))
        );
    }

    /// Cells are whole squares from the image's origin, valued by everything they cover: a cell
    /// the area only touches is the mean of the whole cell, clipped at the image's edge.
    #[test]
    fn a_grid_cell_is_the_mean_of_the_whole_cell() {
        let (w, h) = (20u32, 10u32);
        let source: Vec<u8> = (0..w * h).map(|i| (i * 13 % 97) as u8).collect();
        let mut px = source.clone();
        let mut image = ImageMut::new(w, h, Layout::Gray, Pixels::U8(&mut px)).unwrap();
        let region = Region {
            grid: Some(MosaicGrid::new(6)),
            ..Region::new(rect(5.0, 5.0, 10.0, 5.0))
        };
        render(
            &mut image,
            &[region],
            &style(Method::Mosaic, Shape::Rectangle, 0.0, false),
            None,
        )
        .unwrap();
        let src = &source;
        let mean = |x0: u32, y0: u32| {
            let (x1, y1) = ((x0 + 6).min(w), (y0 + 6).min(h));
            let sum: u32 = (y0..y1)
                .flat_map(|y| (x0..x1).map(move |x| u32::from(src[(y * w + x) as usize])))
                .sum();
            let n = (x1 - x0) * (y1 - y0);
            ((sum + n / 2) / n) as u8
        };
        for y in 0..h {
            for x in 0..w {
                let inside = (5..15).contains(&x) && (5..10).contains(&y);
                let want = if inside {
                    mean(x / 6 * 6, y / 6 * 6)
                } else {
                    source[(y * w + x) as usize]
                };
                assert_eq!(px[(y * w + x) as usize], want, "({x}, {y})");
            }
        }
    }

    /// A track's cell gives each of its boxes at most the strength's cells along the padded
    /// area's short side, and is never under 2 pixels.
    #[test]
    fn a_tracks_cell_is_never_finer_than_the_strength() {
        for strength in [MosaicStrength::MIN, 8, MosaicStrength::MAX] {
            let s = MaskStyle {
                mosaic_strength: MosaicStrength::new(strength).unwrap(),
                ..style(Method::Mosaic, Shape::Rectangle, 0.18, false)
            };
            for side in [1.0, 3.0, 17.0, 64.0, 333.0] {
                let b = rect(10.0, 10.0, side, side * 1.3);
                let cell = MosaicGrid::cell_for(&b, &s, 1000, 1000);
                let padded = padded_region(&b, 0.18, 1000, 1000);
                let short = padded.w.min(padded.h) as u32;
                assert!(cell * u32::from(strength) >= short, "{side} {strength}");
                assert!(cell >= MosaicGrid::MIN_CELL);
            }
        }
        assert_eq!(MosaicGrid::new(0).cell(), MosaicGrid::MIN_CELL);
    }

    #[test]
    fn every_method_handles_tiny_and_border_regions() {
        let overlay = Overlay::new(1, 1, vec![255, 0, 0, 255]).unwrap();
        for layout in [Layout::Gray, Layout::GrayAlpha, Layout::Rgb, Layout::Rgba] {
            for method in [
                Method::Fill,
                Method::Mosaic,
                Method::Blur,
                Method::CustomImage,
            ] {
                for soft in [false, true] {
                    let mut px: Vec<u8> = (0..5 * 4 * layout.channels()).map(|i| i as u8).collect();
                    let before = px.clone();
                    let mut image = ImageMut::new(5, 4, layout, Pixels::U8(&mut px)).unwrap();
                    let regions = [region(0.0, 0.0, 1.0, 1.0), region(4.5, 3.5, 3.0, 3.0)];
                    let s = style(method, Shape::Ellipse, 0.0, soft);
                    render(&mut image, &regions, &s, Some(&overlay)).unwrap();
                    assert_eq!(px.len(), before.len());
                }
            }
        }
    }

    #[test]
    fn a_mosaic_cell_is_the_mean_of_its_pixels() {
        // 4x2 region at strength 4: the short side allows one 2 px cell, the long side two.
        let mut px = vec![0u8, 2, 4, 6, 8, 10, 12, 15];
        let mut image = ImageMut::new(4, 2, Layout::Gray, Pixels::U8(&mut px)).unwrap();
        let s = MaskStyle {
            method: Method::Mosaic,
            padding: Padding::new(0.0).unwrap(),
            mosaic_strength: MosaicStrength::new(4).unwrap(),
            ..MaskStyle::default()
        };
        render(&mut image, &[region(0.0, 0.0, 4.0, 2.0)], &s, None).unwrap();
        // (0 + 2 + 8 + 10) / 4 = 5; (4 + 6 + 12 + 15) / 4 = 9.25.
        assert_eq!(px, [5, 5, 9, 9, 5, 5, 9, 9]);
    }

    #[test]
    fn blur_reaches_past_the_region_edge() {
        // A bright box inside a dark image: its edge pixels average in the dark surroundings.
        let (w, h) = (40u32, 40u32);
        let mut px = vec![0u8; (w * h) as usize];
        for y in 10..30 {
            for x in 10..30 {
                px[y * 40 + x] = 250;
            }
        }
        let mut image = ImageMut::new(w, h, Layout::Gray, Pixels::U8(&mut px)).unwrap();
        let s = style(Method::Blur, Shape::Rectangle, 0.0, false);
        render(&mut image, &[region(10.0, 10.0, 20.0, 20.0)], &s, None).unwrap();
        assert!(px[10 * 40 + 10] < 150, "corner {}", px[10 * 40 + 10]);
        assert!(px[20 * 40 + 20] > px[10 * 40 + 10]);
        assert_eq!(px[0], 0);
    }

    #[test]
    fn box_radii_match_the_requested_sigma() {
        for sigma in [4.0f32, 7.5, 33.0, 100.0] {
            let variance: f32 = box_radii(sigma)
                .iter()
                .map(|&r| {
                    let w = (2 * r + 1) as f32;
                    (w * w - 1.0) / 12.0
                })
                .sum();
            assert!((variance.sqrt() - sigma).abs() / sigma < 0.1, "{sigma}");
        }
    }

    #[test]
    fn invalid_inputs_are_refused() {
        let mut px = vec![0u8; 5];
        assert_eq!(
            ImageMut::new(2, 2, Layout::Gray, Pixels::U8(&mut px)).err(),
            Some(MaskError::BufferSize)
        );
        let mut px = vec![0u8; 4];
        let mut image = ImageMut::new(2, 2, Layout::Gray, Pixels::U8(&mut px)).unwrap();
        let s = style(Method::CustomImage, Shape::Rectangle, 0.0, false);
        assert_eq!(
            render(&mut image, &[], &s, None),
            Err(MaskError::MissingOverlay)
        );
        assert_eq!(MosaicStrength::new(3), Err(MaskError::StrengthOutOfRange));
        // 12 cells across was the weakest mosaic 1.12.1 could make; more cells would hide less.
        assert!(MosaicStrength::new(12).is_ok());
        assert_eq!(MosaicStrength::new(13), Err(MaskError::StrengthOutOfRange));
        assert_eq!(Padding::new(f32::NAN), Err(MaskError::PaddingOutOfRange));
        assert_eq!(Overlay::new(2, 1, vec![0; 4]), Err(MaskError::BufferSize));
    }
}
