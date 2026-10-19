//! The license-plate YOLOv9-t end2end model (open-image-models), prepared and decoded as 1.12.1
//! `PlateDetector` does so results match its fixtures.

use cloakframe_core::detection::{Detection, ModelId, Thresholds, non_max_suppression};
use cloakframe_core::geometry::Rect;
use cloakframe_core::review::Digest;
use ort::session::Session;
use ort::value::{Tensor, TensorElementType, TensorRef, ValueType};
use sha2::{Digest as _, Sha256};

use super::resize::resize_linear;
use super::runtime::{Backend, OrtRuntime};
use super::{DetectError, Detector, Found, Rgb8Image};

const SIZE: usize = 512;
/// Batch index, x1, y1, x2, y2, class, score.
const COLS: usize = 7;
const PAD_VALUE: u8 = 114;
/// The boxes the graph's `NonMaxSuppression` gets: 64² + 32² + 16² anchors of the 512 input.
const ANCHORS: i64 = 5_376;
/// The graph's `NonMaxSuppression` limit (100) and IoU threshold (0.45).
const NMS_MAX_BOXES: &str = "/end2end/Constant_24_output_0";
const NMS_IOU: &str = "/end2end/Constant_25_output_0";

/// The published name of the model file.
pub const FILE_NAME: &str = "yolo-v9-t-512-license-plates-end2end.onnx";

/// The model file this detector runs (`manifests/native.toml`). Sessions are made only from bytes
/// with this hash (plan 4.6).
pub const MODEL_SHA256: &str = "746fdd358ec110418775d7c9d8d07910d48b1a21471f92bf4421f6510d6daade";

pub fn model_digest() -> Digest {
    MODEL_SHA256.parse().expect("a valid SHA-256")
}

pub struct Plate {
    session: Session,
}

impl Plate {
    pub fn new(runtime: &OrtRuntime, model: &[u8], backend: Backend) -> Result<Self, DetectError> {
        if Digest(Sha256::digest(model).into()) != model_digest() {
            return Err(unexpected("file is not the pinned one (SHA-256 differs)"));
        }
        // The graph keeps at most 100 plates and suppresses at IoU 0.45. Both are lifted, so every
        // box above the graph's own 0.001 score floor comes out and NMS runs here with the plate
        // thresholds: a bounded output would leave plates the model found unmasked (D11).
        let max_boxes = Tensor::from_array(([1usize], vec![ANCHORS])).map_err(failed)?;
        let no_suppression = Tensor::from_array(([1usize], vec![1.0f32])).map_err(failed)?;
        let session = runtime
            .session(
                model,
                backend,
                vec![
                    (NMS_MAX_BOXES, max_boxes.into_dyn()),
                    (NMS_IOU, no_suppression.into_dyn()),
                ],
            )
            .map_err(failed)?;
        let (inputs, outputs) = (session.inputs(), session.outputs());
        if inputs.len() != 1 || outputs.len() != 1 {
            return Err(unexpected("not one input and one output"));
        }
        check_tensor(inputs[0].dtype(), &[1, 3, SIZE as i64, SIZE as i64])?;
        check_tensor(outputs[0].dtype(), &[-1, COLS as i64])?;
        Ok(Self { session })
    }
}

fn failed(e: ort::Error) -> DetectError {
    DetectError::Failed(e.to_string())
}

fn unexpected(what: &str) -> DetectError {
    DetectError::UnexpectedModel(format!("license-plate model {what}"))
}

/// A float tensor whose known dimensions match; dynamic ones (`-1`) are accepted.
fn check_tensor(dtype: &ValueType, want: &[i64]) -> Result<(), DetectError> {
    let ValueType::Tensor { ty, shape, .. } = dtype else {
        return Err(unexpected("value is not a tensor"));
    };
    let fits = shape.len() == want.len()
        && shape
            .iter()
            .zip(want)
            .all(|(&d, &w)| d <= 0 || w <= 0 || d == w);
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

/// The NCHW RGB input in 0..=1: the image scaled to fit 512x512 with the `INTER_LINEAR` port,
/// on gray 114. Unlike the face models, 1.12.1 rounds the scaled size and places it at
/// `round(pad - 0.1)`.
fn prepare(image: &Rgb8Image) -> (Vec<f32>, Letterbox) {
    let scale = (SIZE as f32 / image.width as f32).min(SIZE as f32 / image.height as f32);
    let rw = ((image.width as f32 * scale).round() as usize).max(1);
    let rh = ((image.height as f32 * scale).round() as usize).max(1);
    let pad_x = (SIZE - rw) as f32 / 2.0;
    let pad_y = (SIZE - rh) as f32 / 2.0;
    let (left, top) = (
        (pad_x - 0.1).round() as usize,
        (pad_y - 0.1).round() as usize,
    );
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

/// One output row as a plate in image coordinates, or `None` when 1.12.1 skipped it: below the
/// score threshold, not finite, or under one pixel after clamping to the image.
fn decode_row(
    row: &[f32; COLS],
    lb: Letterbox,
    image: &Rgb8Image,
    min_score: f32,
) -> Result<Option<Detection>, DetectError> {
    let score = row[6];
    if !score.is_finite() || score < min_score {
        return Ok(None);
    }
    let to_x = |v: f32| (v - lb.pad_x) / lb.scale;
    let to_y = |v: f32| (v - lb.pad_y) / lb.scale;
    let (x1, y1, x2, y2) = (to_x(row[1]), to_y(row[2]), to_x(row[3]), to_y(row[4]));
    if ![x1, y1, x2, y2].iter().all(|v| v.is_finite()) {
        return Ok(None);
    }
    let (w, h) = (image.width as f32, image.height as f32);
    let (left, top) = (x1.min(x2).clamp(0.0, w), y1.min(y2).clamp(0.0, h));
    let (right, bottom) = (x1.max(x2).clamp(0.0, w), y1.max(y2).clamp(0.0, h));
    if right - left < 1.0 || bottom - top < 1.0 {
        return Ok(None);
    }
    let bbox = Rect::new(left, top, right - left, bottom - top).expect("finite, positive size");
    Detection::new(ModelId::PlateYolov9T, score, bbox, None, None)
        .map(Some)
        .map_err(|e| unexpected(&format!("output: {e}")))
}

impl Detector for Plate {
    fn detect(&mut self, image: &Rgb8Image, thresholds: &Thresholds) -> Result<Found, DetectError> {
        if image.width == 0 || image.height == 0 {
            return Ok(Found::default());
        }
        let min_score = thresholds.of(ModelId::PlateYolov9T.kind()).score();
        let (input, lb) = prepare(image);
        let tensor = TensorRef::from_array_view(([1usize, 3, SIZE, SIZE], input.as_slice()))
            .map_err(failed)?;
        let outputs = self.session.run(ort::inputs![tensor]).map_err(failed)?;
        let (shape, data) = outputs[0].try_extract_tensor::<f32>().map_err(failed)?;
        let rows = shape.first().copied().unwrap_or(-1);
        if shape.len() != 2 || shape[1] != COLS as i64 || !(0..=ANCHORS).contains(&rows) {
            return Err(unexpected(&format!("output shape {shape:?}")));
        }
        let mut candidates = Vec::new();
        for row in data.as_chunks::<COLS>().0 {
            if let Some(d) = decode_row(row, lb, image, min_score)? {
                candidates.push(d);
            }
        }
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
    fn letterbox_rounds_the_size_and_the_offset() {
        let (input, lb) = prepare(&gray(1920, 1080, 0));
        // 1080 * 512 / 1920 = 288 rows, padded by 112 above.
        assert_eq!(
            lb,
            Letterbox {
                scale: 512.0 / 1920.0,
                pad_x: 0.0,
                pad_y: 112.0
            }
        );
        let fill = 114.0f32 * (1.0 / 255.0);
        let plane = SIZE * SIZE;
        for c in 0..3 {
            assert_eq!(input[c * plane + 111 * SIZE], fill);
            assert_eq!(input[c * plane + 112 * SIZE], 0.0);
            assert_eq!(input[c * plane + 399 * SIZE + 511], 0.0);
            assert_eq!(input[c * plane + 400 * SIZE], fill);
        }
        // 333 * 512 / 500 = 340.99 rounds to 341: pad 85.5, placed at round(85.4) = 85.
        let (input, lb) = prepare(&gray(500, 333, 0));
        assert_eq!(lb.pad_y, 85.5);
        assert_eq!(input[84 * SIZE], fill);
        assert_eq!(input[85 * SIZE], 0.0);
        assert_eq!(input[425 * SIZE], 0.0);
        assert_eq!(input[426 * SIZE], fill);
    }

    fn row(x1: f32, y1: f32, x2: f32, y2: f32, score: f32) -> [f32; COLS] {
        [0.0, x1, y1, x2, y2, 0.0, score]
    }

    #[test]
    fn rows_map_back_to_image_coordinates_and_clamp() {
        let image = gray(1920, 1080, 0);
        let lb = Letterbox {
            scale: 0.25,
            pad_x: 0.0,
            pad_y: 112.0,
        };
        let d = decode_row(&row(100.0, 200.0, 120.0, 210.0, 0.9), lb, &image, 0.5)
            .unwrap()
            .unwrap();
        let b = d.bbox();
        assert_eq!(
            (b.x(), b.y(), b.width(), b.height()),
            (400.0, 352.0, 80.0, 40.0)
        );
        assert_eq!((d.model(), d.landmarks()), (ModelId::PlateYolov9T, None));

        // Corners in either order; the part above the image is cut off.
        let d = decode_row(&row(120.0, 120.0, 100.0, 100.0, 0.9), lb, &image, 0.5)
            .unwrap()
            .unwrap();
        let b = d.bbox();
        assert_eq!(
            (b.x(), b.y(), b.width(), b.height()),
            (400.0, 0.0, 80.0, 32.0)
        );
    }

    #[test]
    fn rows_1_12_1_skipped_are_skipped() {
        let image = gray(512, 512, 0);
        let lb = Letterbox {
            scale: 1.0,
            pad_x: 0.0,
            pad_y: 0.0,
        };
        let skip = |r: [f32; COLS]| decode_row(&r, lb, &image, 0.5).unwrap().is_none();
        assert!(!skip(row(10.0, 10.0, 50.0, 30.0, 0.5)));
        assert!(skip(row(10.0, 10.0, 50.0, 30.0, 0.49)));
        assert!(skip(row(10.0, 10.0, 50.0, 30.0, f32::NAN)));
        assert!(skip(row(f32::INFINITY, 10.0, 50.0, 30.0, 0.9)));
        assert!(skip(row(-60.0, 10.0, -20.0, 30.0, 0.9)));
        assert!(skip(row(10.0, 10.0, 10.5, 30.0, 0.9)));
    }

    #[test]
    fn the_pinned_hash_is_the_manifest_entry() {
        let manifest = include_str!("../../../../manifests/native.toml");
        let entry = manifest
            .split("[[artifact]]")
            .find(|e| e.contains("id = \"model-plate-yolov9-t\""))
            .unwrap();
        assert!(entry.contains(&format!("sha256 = \"{MODEL_SHA256}\"")));
    }
}
