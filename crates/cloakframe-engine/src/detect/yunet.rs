//! YuNet 2023mar, decoded as OpenCV's `FaceDetectorYN::postProcess` does (1.12.1 ran it through
//! OpenCV), so results match the 1.12.1 fixtures.

use cloakframe_core::detection::{Detection, ModelId, Thresholds};
use cloakframe_core::geometry::{Point, Rect};
use cloakframe_core::review::Digest;
use ort::session::{Session, SessionOutputs};
use ort::value::TensorRef;
use sha2::{Digest as _, Sha256};

use super::resize::resize_linear;
use super::runtime::OrtRuntime;
use super::{DetectError, Detector, Found, Rgb8Image};

const SIZE: usize = 640;
const STRIDES: [usize; 3] = [8, 16, 32];

/// The published name of the model file.
pub const FILE_NAME: &str = "face_detection_yunet_2023mar.onnx";

/// The model file this detector runs (`manifests/native.toml`). Sessions are made only from bytes
/// with this hash (plan 4.6).
pub const MODEL_SHA256: &str = "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4";

pub fn model_digest() -> Digest {
    MODEL_SHA256.parse().expect("a valid SHA-256")
}

pub struct YuNet {
    session: Session,
}

impl YuNet {
    pub fn new(runtime: &OrtRuntime, model: &[u8]) -> Result<Self, DetectError> {
        if Digest(Sha256::digest(model).into()) != model_digest() {
            return Err(unexpected("file is not the pinned one (SHA-256 differs)"));
        }
        let session = runtime
            .cpu_session(model)
            .map_err(|e| DetectError::Failed(e.to_string()))?;
        let names: Vec<&str> = session.outputs().iter().map(|o| o.name()).collect();
        for stride in STRIDES {
            for head in ["cls", "obj", "bbox", "kps"] {
                let name = format!("{head}_{stride}");
                if !names.contains(&name.as_str()) {
                    return Err(unexpected(&format!("output {name} is missing")));
                }
            }
        }
        if session.inputs().len() != 1 {
            return Err(unexpected("not one input"));
        }
        Ok(Self { session })
    }
}

fn unexpected(what: &str) -> DetectError {
    DetectError::UnexpectedModel(format!("YuNet {what}"))
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Letterbox {
    scale: f32,
    pad_x: f32,
    pad_y: f32,
}

/// The NCHW BGR input in 0..=255 that `FaceDetectorYN` makes: the image scaled to fit 640x640
/// with the `INTER_LINEAR` port, centred on black.
fn prepare(image: &Rgb8Image) -> (Vec<f32>, Letterbox) {
    let scale = (SIZE as f32 / image.width as f32).min(SIZE as f32 / image.height as f32);
    let rw = ((image.width as f32 * scale) as usize).max(1);
    let rh = ((image.height as f32 * scale) as usize).max(1);
    let pad_x = (SIZE - rw) as f32 / 2.0;
    let pad_y = (SIZE - rh) as f32 / 2.0;
    let (left, top) = (pad_x as usize, pad_y as usize);
    let resized = resize_linear(image, rw as u32, rh as u32);

    let plane = SIZE * SIZE;
    let mut input = vec![0.0; 3 * plane];
    for (y, row) in resized.data.chunks_exact(rw * 3).enumerate() {
        for (x, px) in row.as_chunks::<3>().0.iter().enumerate() {
            let at = (top + y) * SIZE + left + x;
            for (c, &v) in px.iter().rev().enumerate() {
                input[c * plane + at] = f32::from(v);
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

/// One face in input coordinates: box (x, y, w, h), five landmarks, score.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Raw {
    rect: [f32; 4],
    landmarks: [[f32; 2]; 5],
    score: f32,
}

/// The per-stride heads of one image.
struct Heads<'a> {
    cls: &'a [f32],
    obj: &'a [f32],
    bbox: &'a [f32],
    kps: &'a [f32],
}

/// Faces of one stride at or above `min_score`, as `postProcess` decodes them.
fn decode_stride(heads: &Heads<'_>, stride: usize, min_score: f32, out: &mut Vec<Raw>) {
    let cols = SIZE / stride;
    let s = stride as f32;
    for (idx, (&cls, &obj)) in heads.cls.iter().zip(heads.obj).enumerate() {
        let score = (cls.clamp(0.0, 1.0) * obj.clamp(0.0, 1.0)).sqrt();
        if score < min_score {
            continue;
        }
        let (r, c) = ((idx / cols) as f32, (idx % cols) as f32);
        let b = &heads.bbox[idx * 4..idx * 4 + 4];
        let (cx, cy) = ((c + b[0]) * s, (r + b[1]) * s);
        let (w, h) = (b[2].exp() * s, b[3].exp() * s);
        let k = &heads.kps[idx * 10..idx * 10 + 10];
        out.push(Raw {
            rect: [cx - w / 2.0, cy - h / 2.0, w, h],
            landmarks: std::array::from_fn(|n| [(k[2 * n] + c) * s, (k[2 * n + 1] + r) * s]),
            score,
        });
    }
}

/// `cv::dnn::NMSBoxes` as `FaceDetectorYN` calls it: on boxes truncated to integers, keeping
/// scores strictly above the threshold, best first (stable). `FaceDetectorYN` passes only its
/// `top_k` (5,000) best candidates; that drops faces once a photo holds several hundred people,
/// as 1.12.1's limit did for YOLO5Face-n (D11), so every candidate goes in. The 8,400 cells of
/// the three strides bound the count.
fn nms_int(faces: Vec<Raw>, min_score: f32, max_iou: f32) -> Vec<Raw> {
    let rect = |f: &Raw| f.rect.map(|v| v as i32);
    let iou = |a: [i32; 4], b: [i32; 4]| -> f32 {
        let (l, t) = (a[0].max(b[0]), a[1].max(b[1]));
        let (r, bt) = (
            (a[0] + a[2]).min(b[0] + b[2]),
            (a[1] + a[3]).min(b[1] + b[3]),
        );
        let inter = if r > l && bt > t {
            f64::from(r - l) * f64::from(bt - t)
        } else {
            0.0
        };
        let union = f64::from(a[2] * a[3]) + f64::from(b[2] * b[3]) - inter;
        if union == 0.0 {
            0.0
        } else {
            (inter / union) as f32
        }
    };
    let mut order: Vec<Raw> = faces.into_iter().filter(|f| f.score > min_score).collect();
    order.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Raw> = Vec::new();
    for f in order {
        if kept.iter().all(|k| iou(rect(&f), rect(k)) <= max_iou) {
            kept.push(f);
        }
    }
    kept
}

/// A face in image coordinates, or `None` when 1.12.1 dropped it: not finite, or under one pixel
/// after clamping to the image.
fn to_detection(
    f: &Raw,
    lb: Letterbox,
    image: &Rgb8Image,
) -> Result<Option<Detection>, DetectError> {
    let to_x = |v: f32| (v - lb.pad_x) / lb.scale;
    let to_y = |v: f32| (v - lb.pad_y) / lb.scale;
    let corners = [
        to_x(f.rect[0]),
        to_y(f.rect[1]),
        to_x(f.rect[0] + f.rect[2]),
        to_y(f.rect[1] + f.rect[3]),
    ];
    if !f.score.is_finite() || !corners.iter().all(|v| v.is_finite()) {
        return Ok(None);
    }
    let (w, h) = (image.width as f32, image.height as f32);
    let (left, top) = (corners[0].clamp(0.0, w), corners[1].clamp(0.0, h));
    let (right, bottom) = (corners[2].clamp(0.0, w), corners[3].clamp(0.0, h));
    if right - left < 1.0 || bottom - top < 1.0 {
        return Ok(None);
    }
    let bbox = Rect::new(left, top, right - left, bottom - top).expect("finite, positive size");
    let landmarks = f.landmarks.map(|[x, y]| Point {
        x: to_x(x),
        y: to_y(y),
    });
    let landmarks = landmarks
        .iter()
        .all(|p| p.x.is_finite() && p.y.is_finite())
        .then_some(landmarks);
    Detection::new(ModelId::YuNet, f.score, bbox, landmarks, None)
        .map(Some)
        .map_err(|e| unexpected(&format!("output: {e}")))
}

fn head<'a>(
    outputs: &'a SessionOutputs<'_>,
    name: &str,
    len: usize,
) -> Result<&'a [f32], DetectError> {
    let (_, data) = outputs[name]
        .try_extract_tensor::<f32>()
        .map_err(|e| DetectError::Failed(e.to_string()))?;
    if data.len() != len {
        return Err(unexpected(&format!(
            "{name} has {} values, expected {len}",
            data.len()
        )));
    }
    Ok(data)
}

impl Detector for YuNet {
    fn detect(&mut self, image: &Rgb8Image, thresholds: &Thresholds) -> Result<Found, DetectError> {
        if image.width == 0 || image.height == 0 {
            return Ok(Found::default());
        }
        let face = thresholds.of(ModelId::YuNet.kind());
        let (input, lb) = prepare(image);
        let tensor = TensorRef::from_array_view(([1usize, 3, SIZE, SIZE], input.as_slice()))
            .map_err(|e| DetectError::Failed(e.to_string()))?;
        let outputs = self
            .session
            .run(ort::inputs![tensor])
            .map_err(|e| DetectError::Failed(e.to_string()))?;
        let mut faces = Vec::new();
        for stride in STRIDES {
            let cells = (SIZE / stride) * (SIZE / stride);
            let heads = Heads {
                cls: head(&outputs, &format!("cls_{stride}"), cells)?,
                obj: head(&outputs, &format!("obj_{stride}"), cells)?,
                bbox: head(&outputs, &format!("bbox_{stride}"), cells * 4)?,
                kps: head(&outputs, &format!("kps_{stride}"), cells * 10)?,
            };
            decode_stride(&heads, stride, face.score(), &mut faces);
        }
        // OpenCV skips NMS for a single face, and with it the strict score filter.
        if faces.len() > 1 {
            faces = nms_int(faces, face.score(), face.nms_iou());
        }
        let mut detections = Vec::with_capacity(faces.len());
        for f in &faces {
            if let Some(d) = to_detection(f, lb, image)? {
                detections.push(d);
            }
        }
        Ok(Found {
            detections,
            over_limit: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, rgb: [u8; 3]) -> Rgb8Image {
        Rgb8Image {
            width,
            height,
            data: rgb.repeat(width as usize * height as usize),
        }
    }

    #[test]
    fn the_input_is_bgr_on_black_without_scaling_the_values() {
        let (input, lb) = prepare(&image(1920, 1080, [10, 20, 30]));
        assert_eq!(
            lb,
            Letterbox {
                scale: 1.0 / 3.0,
                pad_x: 0.0,
                pad_y: 140.0
            }
        );
        let plane = SIZE * SIZE;
        let at = 300 * SIZE + 300;
        assert_eq!(
            [input[at], input[plane + at], input[2 * plane + at]],
            [30.0, 20.0, 10.0]
        );
        assert_eq!(input[139 * SIZE], 0.0);
    }

    fn raw(x: f32, y: f32, w: f32, h: f32, score: f32) -> Raw {
        Raw {
            rect: [x, y, w, h],
            landmarks: [[x + w / 2.0, y + h / 2.0]; 5],
            score,
        }
    }

    #[test]
    fn strides_decode_as_opencv_does() {
        let cells = (SIZE / 32) * (SIZE / 32);
        let mut cls = vec![0.0; cells];
        let mut obj = vec![0.0; cells];
        let mut bbox = vec![0.0; cells * 4];
        let mut kps = vec![0.0; cells * 10];
        // Row 2, column 3 of the stride-32 grid; score sqrt(0.81 * 1.0) = 0.9.
        let idx = 2 * (SIZE / 32) + 3;
        cls[idx] = 0.81;
        obj[idx] = 1.5;
        bbox[idx * 4..idx * 4 + 4].copy_from_slice(&[0.5, 0.25, 0.0, 1.0f32.ln()]);
        kps[idx * 10] = 0.5;
        cls[0] = 0.04;
        obj[0] = 1.0;
        let heads = Heads {
            cls: &cls,
            obj: &obj,
            bbox: &bbox,
            kps: &kps,
        };
        let mut out = Vec::new();
        decode_stride(&heads, 32, 0.5, &mut out);
        assert_eq!(out.len(), 1);
        let f = out[0];
        assert!((f.score - 0.9).abs() < 1e-6);
        assert_eq!(
            f.rect,
            [(3.5 * 32.0) - 16.0, (2.25 * 32.0) - 16.0, 32.0, 32.0]
        );
        assert_eq!(f.landmarks[0], [3.5 * 32.0, 2.0 * 32.0]);
    }

    #[test]
    fn nms_runs_on_integer_boxes_with_a_strict_score_filter() {
        // As floats these overlap by IoU 0.384 and both stay; truncated to (8, 0, 20, 20) the
        // second overlaps by 0.429 and goes.
        let kept = nms_int(
            vec![
                raw(0.0, 0.0, 20.0, 20.0, 0.9),
                raw(8.9, 0.0, 20.0, 20.0, 0.8),
                raw(100.0, 100.0, 20.0, 20.0, 0.5),
            ],
            0.5,
            0.4,
        );
        assert_eq!(kept, [raw(0.0, 0.0, 20.0, 20.0, 0.9)]);
        let kept = nms_int(
            vec![
                raw(0.0, 0.0, 20.0, 20.0, 0.7),
                raw(50.0, 0.0, 20.0, 20.0, 0.9),
            ],
            0.5,
            0.4,
        );
        assert_eq!(kept.iter().map(|f| f.score).collect::<Vec<_>>(), [0.9, 0.7]);
    }

    #[test]
    fn nms_considers_every_candidate() {
        // 71 x 71 separate faces, more than OpenCV's top_k of 5,000.
        let faces: Vec<Raw> = (0..71 * 71)
            .map(|i| {
                let (x, y) = ((i % 71) as f32 * 30.0, (i / 71) as f32 * 30.0);
                raw(x, y, 20.0, 20.0, 0.6 + (i % 7) as f32 * 0.05)
            })
            .collect();
        assert_eq!(nms_int(faces, 0.5, 0.4).len(), 71 * 71);
    }

    #[test]
    fn faces_map_back_to_the_image_and_clamp() {
        let img = image(1920, 1080, [0, 0, 0]);
        let lb = Letterbox {
            scale: 1.0 / 3.0,
            pad_x: 0.0,
            pad_y: 140.0,
        };
        let d = to_detection(&raw(90.0, 185.0, 20.0, 30.0, 0.9), lb, &img)
            .unwrap()
            .unwrap();
        let b = d.bbox();
        assert_eq!(
            (b.x(), b.y(), b.width(), b.height()),
            (270.0, 135.0, 60.0, 90.0)
        );
        assert_eq!(d.model(), ModelId::YuNet);
        assert!(
            to_detection(&raw(-30.0, 200.0, 20.0, 20.0, 0.9), lb, &img)
                .unwrap()
                .is_none()
        );
        assert!(
            to_detection(&raw(f32::NAN, 200.0, 20.0, 20.0, 0.9), lb, &img)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn the_pinned_hash_is_the_manifest_entry() {
        let manifest = include_str!("../../../../manifests/native.toml");
        let entry = manifest
            .split("[[artifact]]")
            .find(|e| e.contains("id = \"model-yunet\""))
            .unwrap();
        assert!(entry.contains(&format!("sha256 = \"{MODEL_SHA256}\"")));
    }
}
