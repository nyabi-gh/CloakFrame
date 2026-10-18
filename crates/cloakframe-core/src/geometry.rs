use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// Axis-aligned box in source display pixels, after EXIF orientation and video rotation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RectFields", into = "RectFields")]
pub struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryError {
    NonFinite,
    NegativeSize,
}

impl fmt::Display for GeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NonFinite => "coordinate is not a finite number",
            Self::NegativeSize => "width or height is negative",
        })
    }
}

impl std::error::Error for GeometryError {}

#[derive(Serialize, Deserialize)]
struct RectFields {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl TryFrom<RectFields> for Rect {
    type Error = GeometryError;

    fn try_from(f: RectFields) -> Result<Self, Self::Error> {
        Self::new(f.x, f.y, f.width, f.height)
    }
}

impl From<Rect> for RectFields {
    fn from(r: Rect) -> Self {
        Self {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        }
    }
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Result<Self, GeometryError> {
        if ![x, y, width, height].iter().all(|v| v.is_finite()) {
            return Err(GeometryError::NonFinite);
        }
        if width < 0.0 || height < 0.0 {
            return Err(GeometryError::NegativeSize);
        }
        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }

    pub fn x(&self) -> f32 {
        self.x
    }

    pub fn y(&self) -> f32 {
        self.y
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.height
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn area(&self) -> f32 {
        self.width * self.height
    }

    /// Intersection over union, 0 when either box is empty. Same f32 arithmetic as 1.12.1
    /// `intersectionOverUnion`, so NMS keeps the same boxes.
    pub fn iou(&self, other: &Rect) -> f32 {
        let (a, b) = (self.area(), other.area());
        if a <= 0.0 || b <= 0.0 {
            return 0.0;
        }
        let w = (self.right().min(other.right()) - self.x.max(other.x)).max(0.0);
        let h = (self.bottom().min(other.bottom()) - self.y.max(other.y)).max(0.0);
        let intersection = w * h;
        intersection / (a + b - intersection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_finite_and_negative_sizes() {
        assert_eq!(
            Rect::new(f32::NAN, 0.0, 1.0, 1.0),
            Err(GeometryError::NonFinite)
        );
        assert_eq!(
            Rect::new(0.0, 0.0, f32::INFINITY, 1.0),
            Err(GeometryError::NonFinite)
        );
        assert_eq!(
            Rect::new(0.0, 0.0, -1.0, 1.0),
            Err(GeometryError::NegativeSize)
        );
    }

    #[test]
    fn allows_negative_origin_and_empty_size() {
        let r = Rect::new(-4.0, -2.0, 10.0, 0.0).unwrap();
        assert_eq!((r.right(), r.bottom(), r.area()), (6.0, -2.0, 0.0));
    }

    /// tests/fixtures/legacy/tests/testIntersectionOverUnion.json (normative).
    #[test]
    fn iou_of_overlapping_disjoint_and_empty_boxes() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0).unwrap();
        let b = Rect::new(5.0, 0.0, 10.0, 10.0).unwrap();
        assert_eq!(a.iou(&b), 50.0 / 150.0);
        assert_eq!(a.iou(&a), 1.0);
        assert_eq!(a.iou(&Rect::new(100.0, 100.0, 10.0, 10.0).unwrap()), 0.0);
        assert_eq!(a.iou(&Rect::new(0.0, 0.0, 0.0, 0.0).unwrap()), 0.0);
        assert_eq!(a.iou(&Rect::new(10.0, 0.0, 5.0, 5.0).unwrap()), 0.0);
    }
}
