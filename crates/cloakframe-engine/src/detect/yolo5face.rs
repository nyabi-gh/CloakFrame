//! YOLO5Face-n, decoded as 1.12.1 `Yolo5FaceDetector` does so results match its fixtures.

use cloakframe_core::detection::{Detection, ModelId, Thresholds, non_max_suppression};
use cloakframe_core::geometry::{Point, Rect};
use cloakframe_core::review::Digest;
use ort::session::Session;
use ort::value::{TensorElementType, TensorRef, ValueType};
use sha2::{Digest as _, Sha256};

use super::resize::resize_linear;
use super::runtime::{Backend, OrtRuntime};
use super::{DetectError, Detector, Found, Rgb8Image};

const SIZE: usize = 640;
const ROWS: usize = 25_200;
const COLS: usize = 16;
const PAD_VALUE: u8 = 114;

/// The published name of the model file.
pub const FILE_NAME: &str = "yolov5n_face.onnx";

/// The model file this detector runs (`manifests/native.toml`). Sessions are made only from bytes
/// with this hash (plan 4.6).
pub const MODEL_SHA256: &str = "eb244a06e36999db732b317c2b30fa113cd6cfc1a397eaf738f2d6f33c01f640";

pub fn model_digest() -> Digest {
    MODEL_SHA256.parse().expect("a valid SHA-256")
}

pub struct Yolo5Face {
    session: Session,
}

impl Yolo5Face {
    pub fn new(runtime: &OrtRuntime, model: &[u8], backend: Backend) -> Result<Self, DetectError> {
        if Digest(Sha256::digest(model).into()) != model_digest() {
            return Err(unexpected("file is not the pinned one (SHA-256 differs)"));
        }
        let session = runtime
            .session(model, backend, Vec::new())
            .map_err(failed)?;
        let (inputs, outputs) = (session.inputs(), session.outputs());
        if inputs.len() != 1 || outputs.len() != 1 {
            return Err(unexpected("not one input and one output"));
        }
        check_tensor(inputs[0].dtype(), &[1, 3, SIZE as i64, SIZE as i64])?;
        check_tensor(outputs[0].dtype(), &[1, ROWS as i64, COLS as i64])?;
        Ok(Self { session })
    }
}

fn failed(e: ort::Error) -> DetectError {
    DetectError::Failed(e.to_string())
}

fn unexpected(what: &str) -> DetectError {
    DetectError::UnexpectedModel(format!("YOLO5Face-n {what}"))
}

/// A float tensor whose known dimensions match; dynamic ones (`-1`) are accepted.
fn check_tensor(dtype: &ValueType, want: &[i64]) -> Result<(), DetectError> {
    let ValueType::Tensor { ty, shape, .. } = dtype else {
        return Err(unexpected("value is not a tensor"));
    };
    let fits = shape.len() == want.len() && shape.iter().zip(want).all(|(&d, &w)| d <= 0 || d == w);
    if *ty != TensorElementType::Float32 || !fits {
        return Err(unexpected(&format!(
            "tensor is {ty:?} {shape:?}, expected f32 {want:?}"
        )));
    }
    Ok(())
}

/// Where the image sits in the model input.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Letterbox {
    scale: f32,
    pad_x: f32,
    pad_y: f32,
}

/// The NCHW RGB input in 0..=1: the image scaled to fit 640x640 with the `INTER_LINEAR` port,
/// centred on gray 114.
fn prepare(image: &Rgb8Image) -> (Vec<f32>, Letterbox) {
    let scale = (SIZE as f32 / image.width as f32).min(SIZE as f32 / image.height as f32);
    let rw = ((image.width as f32 * scale) as usize).max(1);
    let rh = ((image.height as f32 * scale) as usize).max(1);
    let pad_x = (SIZE - rw) as f32 / 2.0;
    let pad_y = (SIZE - rh) as f32 / 2.0;
    let (left, top) = (pad_x as usize, pad_y as usize);
    let resized = resize_linear(image, rw as u32, rh as u32);

    let plane = SIZE * SIZE;
    let fill = f32::from(PAD_VALUE) * (1.0 / 255.0);
    let mut input = vec![fill; 3 * plane];
    for (y, row) in resized.data.chunks_exact(rw * 3).enumerate() {
        for (x, px) in row.as_chunks::<3>().0.iter().enumerate() {
            let at = (top + y) * SIZE + left + x;
            for (c, &v) in px.iter().enumerate() {
                input[c * plane + at] = f32::from(v) * (1.0 / 255.0);
            }
        }
    }
    (
        input,
        Letterbox {
            scale,
            pad_x,
            pad_y,
        },
    )
}

/// One output row as a detection in image coordinates, or `None` when 1.12.1 skipped it: below
/// the score threshold, not finite, or under one pixel after clamping to the image.
fn decode_row(
    row: &[f32; COLS],
    lb: Letterbox,
    image: &Rgb8Image,
    min_score: f32,
) -> Result<Option<Detection>, DetectError> {
    // Objectness alone: the single class score (row[15]) is a near-constant 0.98.
    let score = row[4];
    if !score.is_finite() || score < min_score {
        return Ok(None);
    }
    let to_x = |v: f32| (v - lb.pad_x) / lb.scale;
    let to_y = |v: f32| (v - lb.pad_y) / lb.scale;
    let corners = [
        to_x(row[0] - row[2] * 0.5),
        to_y(row[1] - row[3] * 0.5),
        to_x(row[0] + row[2] * 0.5),
        to_y(row[1] + row[3] * 0.5),
    ];
    if !corners.iter().all(|v| v.is_finite()) {
        return Ok(None);
    }
    let (w, h) = (image.width as f32, image.height as f32);
    let (left, top) = (corners[0].clamp(0.0, w), corners[1].clamp(0.0, h));
    let (right, bottom) = (corners[2].clamp(0.0, w), corners[3].clamp(0.0, h));
    if right - left < 1.0 || bottom - top < 1.0 {
        return Ok(None);
    }
    let bbox = Rect::new(left, top, right - left, bottom - top).expect("finite, positive size");
    let landmarks: [Point; 5] = std::array::from_fn(|p| Point {
        x: to_x(row[5 + p * 2]),
        y: to_y(row[6 + p * 2]),
    });
    let landmarks = landmarks
        .iter()
        .all(|p| p.x.is_finite() && p.y.is_finite())
        .then_some(landmarks);
    Detection::new(ModelId::Yolo5FaceN, score, bbox, landmarks, None)
        .map(Some)
        .map_err(|e| unexpected(&format!("output: {e}")))
}

impl Detector for Yolo5Face {
    fn detect(&mut self, image: &Rgb8Image, thresholds: &Thresholds) -> Result<Found, DetectError> {
        if image.width == 0 || image.height == 0 {
            return Ok(Found::default());
        }
        let min_score = thresholds.of(ModelId::Yolo5FaceN.kind()).score();
        let (input, lb) = prepare(image);
        let tensor = TensorRef::from_array_view(([1usize, 3, SIZE, SIZE], input.as_slice()))
            .map_err(failed)?;
        let outputs = self.session.run(ort::inputs![tensor]).map_err(failed)?;
        let (shape, data) = outputs[0].try_extract_tensor::<f32>().map_err(failed)?;
        if **shape != [1, ROWS as i64, COLS as i64] || data.len() != ROWS * COLS {
            return Err(unexpected(&format!("output shape {shape:?}")));
        }

        // Every row goes to NMS. 1.12.1 kept only the 2,000 best-scored candidates, which lost 21
        // of the 280 faces in the D11 crowd fixture; the fixed 25,200 rows bound NMS instead
        // (about 0.3 s in the worst case of no overlap at all).
        let mut candidates = Vec::new();
        for row in data.as_chunks::<COLS>().0 {
            if let Some(d) = decode_row(row, lb, image, min_score)? {
                candidates.push(d);
            }
        }
        // No limit after NMS either: 1.12.1 dropped survivors beyond 300, leaving faces it had
        // found unmasked. Masking cost follows the masked area, not the box count.
        Ok(Found {
            detections: non_max_suppression(candidates, thresholds),
            ..Found::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray(width: u32, height: u32, value: u8) -> Rgb8Image {
        Rgb8Image {
            width,
            height,
            data: vec![value; width as usize * height as usize * 3],
        }
    }

    #[test]
    fn letterbox_centres_the_scaled_image() {
        let (input, lb) = prepare(&gray(1920, 1080, 0));
        assert_eq!(
            lb,
            Letterbox {
                scale: 1.0 / 3.0,
                pad_x: 0.0,
                pad_y: 140.0
            }
        );
        let fill = 114.0f32 * (1.0 / 255.0);
        let plane = SIZE * SIZE;
        for c in 0..3 {
            assert_eq!(input[c * plane + 139 * SIZE], fill);
            assert_eq!(input[c * plane + 140 * SIZE], 0.0);
            assert_eq!(input[c * plane + 499 * SIZE + 639], 0.0);
            assert_eq!(input[c * plane + 500 * SIZE], fill);
        }
        // Odd padding: 1.12.1 truncates the offset and keeps the fractional pad for decoding.
        let (_, lb) = prepare(&gray(640, 427, 0));
        assert_eq!((lb.scale, lb.pad_y), (1.0, 106.5));
    }

    fn row(cx: f32, cy: f32, w: f32, h: f32, score: f32) -> [f32; COLS] {
        let mut r = [0.0; COLS];
        r[..5].copy_from_slice(&[cx, cy, w, h, score]);
        for p in 0..5 {
            r[5 + p * 2] = cx - w / 4.0 + (p % 2) as f32 * w / 2.0;
            r[6 + p * 2] = cy;
        }
        r
    }

    #[test]
    fn rows_map_back_to_image_coordinates_and_clamp() {
        let image = gray(1920, 1080, 0);
        let lb = Letterbox {
            scale: 1.0 / 3.0,
            pad_x: 0.0,
            pad_y: 140.0,
        };
        let d = decode_row(&row(100.0, 200.0, 20.0, 30.0, 0.9), lb, &image, 0.5)
            .unwrap()
            .unwrap();
        let b = d.bbox();
        assert_eq!(
            (b.x(), b.y(), b.width(), b.height()),
            (270.0, 135.0, 60.0, 90.0)
        );
        assert!(d.landmarks().is_some());

        let edge = decode_row(&row(0.0, 140.0, 20.0, 20.0, 0.9), lb, &image, 0.5)
            .unwrap()
            .unwrap();
        assert_eq!((edge.bbox().x(), edge.bbox().y()), (0.0, 0.0));
        assert_eq!((edge.bbox().width(), edge.bbox().height()), (30.0, 30.0));
    }

    #[test]
    fn the_pinned_hash_is_the_manifest_entry() {
        let manifest = include_str!("../../../../manifests/native.toml");
        let entry = manifest
            .split("[[artifact]]")
            .find(|e| e.contains("version = \"yolov5n_face\""))
            .unwrap();
        assert!(entry.contains(&format!("sha256 = \"{MODEL_SHA256}\"")));
    }

    #[test]
    fn rows_1_12_1_skipped_are_skipped() {
        let image = gray(640, 640, 0);
        let lb = Letterbox {
            scale: 1.0,
            pad_x: 0.0,
            pad_y: 0.0,
        };
        let skip = |r: [f32; COLS]| decode_row(&r, lb, &image, 0.5).unwrap().is_none();
        assert!(skip(row(100.0, 100.0, 20.0, 20.0, 0.49)));
        assert!(skip(row(100.0, 100.0, 20.0, 20.0, f32::NAN)));
        assert!(skip(row(f32::INFINITY, 100.0, 20.0, 20.0, 0.9)));
        assert!(skip(row(-30.0, 100.0, 20.0, 20.0, 0.9)));
        assert!(skip(row(100.0, 100.0, 0.5, 20.0, 0.9)));

        let mut no_landmarks = row(100.0, 100.0, 20.0, 20.0, 0.9);
        no_landmarks[7] = f32::NAN;
        let d = decode_row(&no_landmarks, lb, &image, 0.5).unwrap().unwrap();
        assert_eq!((d.landmarks(), d.roll()), (None, None));
    }
}
