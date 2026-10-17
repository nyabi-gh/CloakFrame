use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// Axis-aligned box in source display pixels, after EXIF orientation and video rotation.
#[derive(Debug, Clone, Copy, PartialEq)]
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
}
