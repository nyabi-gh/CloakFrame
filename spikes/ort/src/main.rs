use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, ensure};
use ort::environment::Environment;
use ort::ep;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::TensorRef;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct Bgr {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

impl Bgr {
    fn load_png(path: &Path) -> Result<Self> {
        let rgb = image::open(path).with_context(|| format!("{}", path.display()))?.to_rgb8();
        let (width, height) = (rgb.width() as usize, rgb.height() as usize);
        let mut data = rgb.into_raw();
        for px in data.chunks_exact_mut(3) {
            px.swap(0, 2);
        }
        Ok(Self { width, height, data })
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct Detection {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    score: f32,
    roll: f32,
    has_pose: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct Record {
    model: String,
    image: String,
    omitted: i32,
    detections: Vec<Detection>,
}

// Mirrors OpenCV 5.0 cv::resize(INTER_LINEAR) on 8-bit input (docs/M0_ORT_SPIKE.md):
// 11-bit coefficients rounded half to even like saturate_cast<short>, fx pinned at the
// horizontal borders but fy kept with only the row index clamped, and the 128-bit vertical
// pass ((S >> 4) * b >> 16, summed, + 2 >> 2) for the whole row once it has 8 elements, since
// OpenCV overlaps its last vector instead of finishing with the scalar formula.
fn resize_linear_cv(src: &Bgr, dw: usize, dh: usize) -> Bgr {
    const SCALE: f32 = 2048.0;
    let cn = 3;
    let scale_x = src.width as f64 / dw as f64;
    let scale_y = src.height as f64 / dh as f64;

    let mut xofs = vec![0usize; dw];
    let mut alpha = vec![[0i32; 2]; dw];
    for dx in 0..dw {
        let mut fx = ((dx as f64 + 0.5) * scale_x - 0.5) as f32;
        let mut sx = fx.floor() as i64;
        fx -= sx as f32;
        if sx < 0 {
            fx = 0.0;
            sx = 0;
        }
        if sx >= src.width as i64 - 1 {
            fx = 0.0;
            sx = src.width as i64 - 1;
        }
        xofs[dx] = sx as usize;
        alpha[dx] = [((1.0 - fx) * SCALE).round_ties_even() as i32, (fx * SCALE).round_ties_even() as i32];
    }

    let hrow = |sy: usize| -> Vec<i32> {
        let row = &src.data[sy * src.width * cn..(sy + 1) * src.width * cn];
        let mut out = vec![0i32; dw * cn];
        for dx in 0..dw {
            let sx = xofs[dx];
            for c in 0..cn {
                let a = row[sx * cn + c] as i32;
                let b = if sx + 1 < src.width { row[(sx + 1) * cn + c] as i32 } else { 0 };
                out[dx * cn + c] = a * alpha[dx][0] + b * alpha[dx][1];
            }
        }
        out
    };

    let width = dw * cn;
    let vectorized = width >= 8;
    let mut data = vec![0u8; dh * width];
    for dy in 0..dh {
        let fy_full = ((dy as f64 + 0.5) * scale_y - 0.5) as f32;
        let sy = fy_full.floor() as i64;
        let fy = fy_full - sy as f32;
        let b0 = ((1.0 - fy) * SCALE).round_ties_even() as i32;
        let b1 = (fy * SCALE).round_ties_even() as i32;
        let clamp = |v: i64| v.clamp(0, src.height as i64 - 1) as usize;
        let r0 = hrow(clamp(sy));
        let r1 = hrow(clamp(sy + 1));
        let out = &mut data[dy * width..(dy + 1) * width];
        for x in 0..width {
            let v = if vectorized {
                (((b0 * (r0[x] >> 4)) >> 16) + ((b1 * (r1[x] >> 4)) >> 16) + 2) >> 2
            } else {
                (r0[x] * b0 + r1[x] * b1 + (1 << 21)) >> 22
            };
            out[x] = v.clamp(0, 255) as u8;
        }
    }
    Bgr { width: dw, height: dh, data }
}

fn letterbox(src: &Bgr, cw: usize, ch: usize, left: usize, top: usize, fill: u8) -> Bgr {
    let mut data = vec![fill; cw * ch * 3];
    for y in 0..src.height {
        let dst = ((top + y) * cw + left) * 3;
        data[dst..dst + src.width * 3].copy_from_slice(&src.data[y * src.width * 3..(y + 1) * src.width * 3]);
    }
    Bgr { width: cw, height: ch, data }
}

fn blob(img: &Bgr, scale: f32, swap_rb: bool) -> Vec<f32> {
    let plane = img.width * img.height;
    let mut out = vec![0f32; plane * 3];
    for i in 0..plane {
        for c in 0..3 {
            let src_c = if swap_rb { 2 - c } else { c };
            out[c * plane + i] = img.data[i * 3 + src_c] as f32 * scale;
        }
    }
    out
}

fn face_roll(lm: &[(f32, f32); 5], b: &Detection) -> Option<f32> {
    if b.w <= 0.0 || b.h <= 0.0 {
        return None;
    }
    let (mx, my) = (b.w * 0.35, b.h * 0.35);
    for &(x, y) in lm {
        if !x.is_finite() || !y.is_finite() || x < b.x - mx || x > b.x + b.w + mx || y < b.y - my || y > b.y + b.h + my {
            return None;
        }
    }
    let angle_for = |l: usize, r: usize| -> Option<f32> {
        let dx = lm[r].0 - lm[l].0;
        let dy = lm[r].1 - lm[l].1;
        let distance = dx.hypot(dy);
        if dx <= 0.0 || distance < b.w * 0.08 || distance > b.w * 1.25 {
            return None;
        }
        let angle = dy.atan2(dx);
        (angle.abs() <= 1.047_197_6).then_some(angle)
    };
    let eye = angle_for(0, 1)?;
    match angle_for(3, 4) {
        Some(mouth) if (mouth - eye).abs() <= 0.45 => Some((eye.sin() + mouth.sin()).atan2(eye.cos() + mouth.cos())),
        _ => Some(eye),
    }
}

fn clamp_box(x1: f32, y1: f32, x2: f32, y2: f32, img: &Bgr) -> Option<(f32, f32, f32, f32)> {
    if !(x1.is_finite() && y1.is_finite() && x2.is_finite() && y2.is_finite()) {
        return None;
    }
    let (w, h) = (img.width as f32, img.height as f32);
    let l = x1.clamp(0.0, w);
    let t = y1.clamp(0.0, h);
    let r = x2.clamp(0.0, w);
    let b = y2.clamp(0.0, h);
    (r - l >= 1.0 && b - t >= 1.0).then_some((l, t, r - l, b - t))
}

fn iou(a: &Detection, b: &Detection) -> f32 {
    let (aa, ba) = (a.w * a.h, b.w * b.h);
    if aa <= 0.0 || ba <= 0.0 {
        return 0.0;
    }
    let l = a.x.max(b.x);
    let t = a.y.max(b.y);
    let r = (a.x + a.w).min(b.x + b.w);
    let bt = (a.y + a.h).min(b.y + b.h);
    let inter = (r - l).max(0.0) * (bt - t).max(0.0);
    inter / (aa + ba - inter)
}

fn nms(mut dets: Vec<Detection>, threshold: f32) -> Vec<Detection> {
    dets.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut suppressed = vec![false; dets.len()];
    let mut kept = Vec::new();
    for i in 0..dets.len() {
        if suppressed[i] {
            continue;
        }
        kept.push(dets[i]);
        for j in i + 1..dets.len() {
            if !suppressed[j] && iou(&dets[i], &dets[j]) > threshold {
                suppressed[j] = true;
            }
        }
    }
    kept
}

struct Yolo5Face(Session);

impl Yolo5Face {
    const SIZE: usize = 640;
    const ROWS: usize = 25_200;
    const COLS: usize = 16;

    fn detect(&mut self, img: &Bgr, score_thr: f32, nms_thr: f32) -> Result<Record> {
        let scale = (Self::SIZE as f32 / img.width as f32).min(Self::SIZE as f32 / img.height as f32);
        let rw = ((img.width as f32 * scale) as usize).max(1);
        let rh = ((img.height as f32 * scale) as usize).max(1);
        let pad_x = (Self::SIZE - rw) as f32 / 2.0;
        let pad_y = (Self::SIZE - rh) as f32 / 2.0;
        let canvas = letterbox(&resize_linear_cv(img, rw, rh), Self::SIZE, Self::SIZE, pad_x as usize, pad_y as usize, 114);
        let input = blob(&canvas, 1.0 / 255.0, true);
        let outputs = self.0.run(ort::inputs![TensorRef::from_array_view(([1usize, 3, Self::SIZE, Self::SIZE], input.as_slice()))?])?;
        let (shape, data) = outputs[0].try_extract_tensor::<f32>()?;
        ensure!(**shape == [1, Self::ROWS as i64, Self::COLS as i64], "unexpected YOLO5Face output {shape:?}");

        let mut candidates = Vec::new();
        for row in data.chunks_exact(Self::COLS) {
            let score = row[4];
            if !score.is_finite() || score < score_thr {
                continue;
            }
            let Some((x, y, w, h)) = clamp_box(
                (row[0] - row[2] * 0.5 - pad_x) / scale,
                (row[1] - row[3] * 0.5 - pad_y) / scale,
                (row[0] + row[2] * 0.5 - pad_x) / scale,
                (row[1] + row[3] * 0.5 - pad_y) / scale,
                img,
            ) else {
                continue;
            };
            let mut d = Detection { x, y, w, h, score, roll: 0.0, has_pose: false };
            let lm: [(f32, f32); 5] = std::array::from_fn(|p| ((row[5 + p * 2] - pad_x) / scale, (row[6 + p * 2] - pad_y) / scale));
            if let Some(roll) = face_roll(&lm, &d) {
                d.roll = roll;
                d.has_pose = true;
            }
            candidates.push(d);
        }
        let mut omitted = 0;
        if candidates.len() > 2_000 {
            candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
            omitted += candidates.len() - 2_000;
            candidates.truncate(2_000);
        }
        let mut detections = nms(candidates, nms_thr);
        if detections.len() > 300 {
            omitted += detections.len() - 300;
            detections.truncate(300);
        }
        Ok(Record { model: "yolo5face".into(), image: String::new(), omitted: omitted as i32, detections })
    }
}

struct YuNet(Session);

impl YuNet {
    const SIZE: usize = 640;
    const TOP_K: usize = 5_000;

    fn detect(&mut self, img: &Bgr, score_thr: f32, nms_thr: f32) -> Result<Record> {
        let scale = (Self::SIZE as f32 / img.width as f32).min(Self::SIZE as f32 / img.height as f32);
        let rw = ((img.width as f32 * scale) as usize).max(1);
        let rh = ((img.height as f32 * scale) as usize).max(1);
        let pad_x = (Self::SIZE - rw) as f32 / 2.0;
        let pad_y = (Self::SIZE - rh) as f32 / 2.0;
        let canvas = letterbox(&resize_linear_cv(img, rw, rh), Self::SIZE, Self::SIZE, pad_x as usize, pad_y as usize, 0);
        let input = blob(&canvas, 1.0, false);
        let outputs = self.0.run(ort::inputs!["input" => TensorRef::from_array_view(([1usize, 3, Self::SIZE, Self::SIZE], input.as_slice()))?])?;

        // Raw faces in canvas coordinates: (x, y, w, h, 5 landmark pairs, score), decoded
        // exactly as OpenCV's FaceDetectorYN::postProcess does for the 2023mar model.
        let mut faces: Vec<[f32; 15]> = Vec::new();
        for stride in [8usize, 16, 32] {
            let cols = Self::SIZE / stride;
            let rows = Self::SIZE / stride;
            let get = |name: String, per: usize| -> Result<&[f32]> {
                let (_, data) = outputs[name.as_str()].try_extract_tensor::<f32>()?;
                ensure!(data.len() == rows * cols * per, "unexpected YuNet {name} length {}", data.len());
                Ok(data)
            };
            let cls = get(format!("cls_{stride}"), 1)?;
            let obj = get(format!("obj_{stride}"), 1)?;
            let bbox = get(format!("bbox_{stride}"), 4)?;
            let kps = get(format!("kps_{stride}"), 10)?;
            for r in 0..rows {
                for c in 0..cols {
                    let idx = r * cols + c;
                    let score = (cls[idx].clamp(0.0, 1.0) * obj[idx].clamp(0.0, 1.0)).sqrt();
                    if score < score_thr {
                        continue;
                    }
                    let s = stride as f32;
                    let cx = (c as f32 + bbox[idx * 4]) * s;
                    let cy = (r as f32 + bbox[idx * 4 + 1]) * s;
                    let w = bbox[idx * 4 + 2].exp() * s;
                    let h = bbox[idx * 4 + 3].exp() * s;
                    let mut face = [0f32; 15];
                    face[..4].copy_from_slice(&[cx - w / 2.0, cy - h / 2.0, w, h]);
                    for n in 0..5 {
                        face[4 + 2 * n] = (kps[idx * 10 + 2 * n] + c as f32) * s;
                        face[5 + 2 * n] = (kps[idx * 10 + 2 * n + 1] + r as f32) * s;
                    }
                    face[14] = score;
                    faces.push(face);
                }
            }
        }
        if faces.len() > 1 {
            faces = nms_opencv_int(&faces, score_thr, nms_thr, Self::TOP_K);
        }

        let mut detections = Vec::new();
        for f in faces {
            let score = f[14];
            if !score.is_finite() || score < score_thr {
                continue;
            }
            let Some((x, y, w, h)) = clamp_box(
                (f[0] - pad_x) / scale,
                (f[1] - pad_y) / scale,
                (f[0] + f[2] - pad_x) / scale,
                (f[1] + f[3] - pad_y) / scale,
                img,
            ) else {
                continue;
            };
            let mut d = Detection { x, y, w, h, score, roll: 0.0, has_pose: false };
            let lm: [(f32, f32); 5] = std::array::from_fn(|p| ((f[4 + p * 2] - pad_x) / scale, (f[5 + p * 2] - pad_y) / scale));
            if let Some(roll) = face_roll(&lm, &d) {
                d.roll = roll;
                d.has_pose = true;
            }
            detections.push(d);
        }
        Ok(Record { model: "yunet".into(), image: String::new(), omitted: 0, detections })
    }
}

// cv::dnn::NMSBoxes over Rect2i built by truncating the float boxes, with a strict
// score > threshold filter and a stable descending sort, as FaceDetectorYN calls it.
fn nms_opencv_int(faces: &[[f32; 15]], score_thr: f32, nms_thr: f32, top_k: usize) -> Vec<[f32; 15]> {
    let rect = |f: &[f32; 15]| (f[0] as i32, f[1] as i32, f[2] as i32, f[3] as i32);
    let overlap = |a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)| -> f32 {
        let l = a.0.max(b.0);
        let t = a.1.max(b.1);
        let r = (a.0 + a.2).min(b.0 + b.2);
        let bt = (a.1 + a.3).min(b.1 + b.3);
        let inter = if r > l && bt > t { (r - l) as f64 * (bt - t) as f64 } else { 0.0 };
        let union = (a.2 * a.3) as f64 + (b.2 * b.3) as f64 - inter;
        if union == 0.0 { 0.0 } else { (inter / union) as f32 }
    };
    let mut order: Vec<usize> = (0..faces.len()).filter(|&i| faces[i][14] > score_thr).collect();
    order.sort_by(|&a, &b| faces[b][14].total_cmp(&faces[a][14]));
    order.truncate(top_k);
    let mut kept: Vec<usize> = Vec::new();
    for i in order {
        if kept.iter().all(|&k| overlap(rect(&faces[i]), rect(&faces[k])) <= nms_thr) {
            kept.push(i);
        }
    }
    kept.into_iter().map(|i| faces[i]).collect()
}

struct Plate(Session);

impl Plate {
    const SIZE: usize = 512;

    fn detect(&mut self, img: &Bgr, score_thr: f32) -> Result<Record> {
        let ratio = (Self::SIZE as f32 / img.width as f32).min(Self::SIZE as f32 / img.height as f32);
        let rw = ((img.width as f32 * ratio).round() as usize).max(1);
        let rh = ((img.height as f32 * ratio).round() as usize).max(1);
        let pad_x = (Self::SIZE - rw) as f32 / 2.0;
        let pad_y = (Self::SIZE - rh) as f32 / 2.0;
        let left = (pad_x - 0.1).round() as usize;
        let top = (pad_y - 0.1).round() as usize;
        let resized = if (rw, rh) == (img.width, img.height) { img.clone() } else { resize_linear_cv(img, rw, rh) };
        let canvas = letterbox(&resized, Self::SIZE, Self::SIZE, left, top, 114);
        let input = blob(&canvas, 1.0 / 255.0, true);
        let outputs = self.0.run(ort::inputs![TensorRef::from_array_view(([1usize, 3, Self::SIZE, Self::SIZE], input.as_slice()))?])?;
        let (shape, data) = outputs[0].try_extract_tensor::<f32>()?;
        ensure!(shape.len() == 2 && shape[1] >= 7, "unexpected plate output {shape:?}");
        let stride = shape[1] as usize;
        let mut detections = Vec::new();
        for row in data.chunks_exact(stride) {
            let score = row[6];
            if !score.is_finite() || score < score_thr {
                continue;
            }
            let (x1, y1, x2, y2) = ((row[1] - pad_x) / ratio, (row[2] - pad_y) / ratio, (row[3] - pad_x) / ratio, (row[4] - pad_y) / ratio);
            let Some((x, y, w, h)) = clamp_box(x1.min(x2), y1.min(y2), x1.max(x2), y1.max(y2), img) else {
                continue;
            };
            detections.push(Detection { x, y, w, h, score, roll: 0.0, has_pose: false });
        }
        Ok(Record { model: "plate".into(), image: String::new(), omitted: 0, detections })
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Accel {
    Cpu,
    CoreMl { static_shapes: bool },
    Dml,
}

/// Session options follow 1.12.1 `OrtAcceleration`: CPU at the full optimization level with one
/// intra-op thread per logical core; DirectML sequential, without memory patterns, at the
/// extended level and on one thread, as the DML EP requires.
fn session(env: &Environment, model: &Path, accel: Accel) -> Result<Session> {
    let bytes = std::fs::read(model)?;
    let mut builder = Session::builder(env)?;
    // ort's builder sets the MaxEfficiency device policy (api-22+). ORT 1.31 then adds CoreML by
    // itself even next to an explicit CPU EP, so the policy is reset before naming the EP.
    if std::env::var_os("SPIKE_KEEP_ORT_POLICY").is_none() {
        builder = builder.with_auto_device(ort::session::builder::AutoDevicePolicy::PreferCPU).map_err(ort::Error::<()>::from)?;
    }
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    builder = match accel {
        Accel::Cpu => builder
            .with_optimization_level(GraphOptimizationLevel::All)
            .and_then(|b| b.with_intra_threads(threads))
            .and_then(|b| b.with_execution_providers([ep::CPU::default().build().error_on_failure()])),
        Accel::CoreMl { static_shapes } => builder.with_execution_providers([ep::CoreML::default()
            .with_model_format(ep::coreml::ModelFormat::MLProgram)
            .with_compute_units(ep::coreml::ComputeUnits::All)
            .with_static_input_shapes(static_shapes)
            .with_model_cache_dir(std::env::temp_dir().join("ort-spike-coreml-cache").display())
            .build()
            .error_on_failure()]),
        #[cfg(target_os = "windows")]
        Accel::Dml => builder
            .with_memory_pattern(false)
            .and_then(|b| b.with_parallel_execution(false))
            .and_then(|b| b.with_optimization_level(GraphOptimizationLevel::Level2))
            .and_then(|b| b.with_intra_threads(1))
            .and_then(|b| b.with_execution_providers([ep::DirectML::default().with_device_id(0).build().error_on_failure()])),
        #[cfg(not(target_os = "windows"))]
        Accel::Dml => anyhow::bail!("DirectML is only built on Windows"),
    }
    .map_err(ort::Error::<()>::from)?;
    if let Ok(dir) = std::env::var("SPIKE_PROFILE") {
        let stem = model.file_stem().unwrap().to_string_lossy();
        builder = builder.with_profiling(Path::new(&dir).join(format!("{stem}-{accel:?}"))).map_err(ort::Error::<()>::from)?;
    }
    Ok(builder.commit_from_memory(&bytes)?)
}

struct Models {
    yolo: Yolo5Face,
    yunet: YuNet,
    plate: Plate,
    accel: [Accel; 3],
}

impl Models {
    fn new(env: &Environment, dir: &Path, yolo: Accel, yunet: Accel, plate: Accel) -> Result<Self> {
        Ok(Self {
            yolo: Yolo5Face(session(env, &dir.join("yolov5n_face.onnx"), yolo)?),
            yunet: YuNet(session(env, &dir.join("face_detection_yunet_2023mar.onnx"), yunet)?),
            plate: Plate(session(env, &dir.join("yolo-v9-t-512-license-plates-end2end.onnx"), plate)?),
            accel: [yolo, yunet, plate],
        })
    }

    fn detect(&mut self, img: &Bgr, score: f32) -> Result<[(&'static str, Record); 3]> {
        Ok([
            ("yolo5face", self.yolo.detect(img, score, 0.4)?),
            ("yunet", self.yunet.detect(img, score, 0.4)?),
            ("plate", self.plate.detect(img, score)?),
        ])
    }
}

/// Runs the fixture images on the accelerated sessions for `seconds`. The first failure (for
/// example a GPU reset forced with `dxcap -forcetdr`) switches to CPU sessions: the image that
/// failed is run again on CPU and compared with the fixture, and the rest of the run stays on CPU.
fn fallback_test(env: &Environment, dir: &Path, fixtures: &Path, decoded: &Path, models: &mut Models, images: &[&str], seconds: u64, simulate_after: Option<usize>) -> Result<()> {
    let deadline = Instant::now() + std::time::Duration::from_secs(seconds);
    let mut on_cpu = false;
    let mut runs = [0usize; 2];
    let mut failure = None;
    let mut rerun = None;
    let mut cpu: Option<Models> = None;
    let mut i = 0usize;
    while Instant::now() < deadline {
        let name = images[i % images.len()];
        i += 1;
        let img = Bgr::load_png(&decoded.join(format!("{name}.png")))?;
        if !on_cpu {
            let simulated = simulate_after.is_some_and(|n| runs[0] >= n);
            let result = if simulated { Err(anyhow::anyhow!("simulated accelerator failure")) } else { models.detect(&img, 0.25) };
            match result {
                Ok(_) => runs[0] += 1,
                Err(e) => {
                    let at = Instant::now();
                    println!("{}", serde_json::json!({"event": "accelerator_failure", "image": name, "after_runs": runs[0], "error": format!("{e:#}")}));
                    failure = Some(format!("{e:#}"));
                    let models_cpu = Models::new(env, dir, Accel::Cpu, Accel::Cpu, Accel::Cpu)?;
                    cpu = Some(models_cpu);
                    let records = cpu.as_mut().unwrap().detect(&img, 0.25)?;
                    let mut worst = Worst { min_iou: 1.0, ..Default::default() };
                    for (model, rec) in &records {
                        compare(fixtures, model, name, 0.25, rec, &mut worst)?;
                    }
                    let ok = worst.count_mismatches == 0 && worst.unmatched == 0;
                    println!("{}", serde_json::json!({"event": "cpu_rerun", "image": name, "matches_fixture": ok, "switch_ms": at.elapsed().as_secs_f64() * 1000.0}));
                    rerun = Some(ok);
                    on_cpu = true;
                    runs[1] += 1;
                }
            }
        } else {
            cpu.as_mut().unwrap().detect(&img, 0.25)?;
            runs[1] += 1;
        }
    }
    println!(
        "FALLBACK {}",
        serde_json::json!({"accelerated_runs": runs[0], "cpu_runs": runs[1], "failure": failure, "cpu_rerun_matches_fixture": rerun, "accel": format!("{:?}", models.accel)})
    );
    Ok(())
}

fn compare_resize(decoded: &Path, images: &[&str]) -> Result<()> {
    for name in images {
        let src = Bgr::load_png(&decoded.join(format!("{name}.png")))?;
        for target in [640usize, 512] {
            let reference = Bgr::load_png(&decoded.join(format!("resized_{target}_{name}.png")))?;
            let mine = resize_linear_cv(&src, reference.width, reference.height);
            let off = mine.data.iter().zip(&reference.data).filter(|(a, b)| a != b).count();
            println!("resize {name} -> {}x{}: differing bytes {off}/{}", reference.width, reference.height, mine.data.len());
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct FixtureDetection {
    #[serde(rename = "box")]
    rect: [f32; 4],
    score: f32,
    roll: f32,
    has_pose: bool,
}

#[derive(Deserialize)]
struct Fixture {
    omitted: i32,
    detections: Vec<FixtureDetection>,
}

#[derive(Default, Serialize)]
struct Worst {
    cases: usize,
    count_mismatches: usize,
    unmatched: usize,
    min_iou: f32,
    max_dscore: f32,
    max_droll: f32,
}

fn compare(fixtures: &Path, model: &str, image: &str, score: f32, ours: &Record, worst: &mut Worst) -> Result<()> {
    let path = fixtures.join("detect").join(model).join(format!("{image}@{score:.2}.json"));
    let want: Fixture = serde_json::from_str(&std::fs::read_to_string(&path).with_context(|| path.display().to_string())?)?;
    worst.cases += 1;
    if want.detections.len() != ours.detections.len() {
        worst.count_mismatches += 1;
    }
    for w in &want.detections {
        let w = Detection { x: w.rect[0], y: w.rect[1], w: w.rect[2], h: w.rect[3], score: w.score, roll: w.roll, has_pose: w.has_pose };
        match ours.detections.iter().max_by(|a, b| iou(&w, a).total_cmp(&iou(&w, b))) {
            Some(got) if iou(&w, got) >= 0.5 => {
                worst.min_iou = worst.min_iou.min(iou(&w, got));
                worst.max_dscore = worst.max_dscore.max((w.score - got.score).abs());
                if w.has_pose && got.has_pose {
                    worst.max_droll = worst.max_droll.max((w.roll - got.roll).abs());
                } else if w.has_pose != got.has_pose {
                    worst.max_droll = f32::INFINITY;
                }
            }
            _ => worst.unmatched += 1,
        }
    }
    println!(
        "{model:9} {image:18} @{score:.2} 1.12.1 {:3} (omitted {:4}) rust {:3}",
        want.detections.len(),
        want.omitted,
        ours.detections.len()
    );
    Ok(())
}

/// `ort-spike resize-check <dir>`: compares the INTER_LINEAR port with OpenCV on the pairs
/// listed in `<dir>/index.json` (made by reference/opencv_reference.py).
fn resize_check(dir: &Path) -> Result<()> {
    #[derive(Deserialize)]
    struct Pair {
        src: String,
        target: [usize; 2],
        #[serde(rename = "ref")]
        reference: String,
    }
    let pairs: Vec<Pair> = serde_json::from_str(&std::fs::read_to_string(dir.join("index.json"))?)?;
    let mut failing = Vec::new();
    for pair in &pairs {
        let src = Bgr::load_png(&dir.join(&pair.src))?;
        let reference = Bgr::load_png(&dir.join(&pair.reference))?;
        let mine = resize_linear_cv(&src, pair.target[0], pair.target[1]);
        let off = mine.data.iter().zip(&reference.data).filter(|(a, b)| a != b).count();
        if off > 0 {
            failing.push(serde_json::json!({"src": [src.width, src.height], "target": pair.target, "differing_bytes": off}));
        }
    }
    println!("RESIZE {}", serde_json::json!({"dir": dir.display().to_string(), "pairs": pairs.len(), "mismatching": failing}));
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("resize-check") {
        return resize_check(Path::new(args.get(2).context("dir")?));
    }
    ensure!(
        args.len() >= 5,
        "usage: ort-spike <models dir> <fixtures dir> <decoded png dir> <libonnxruntime> [--coreml [--static-shapes] [--plate-coreml] | --dml [--plate-cpu]] [--fallback-test <s> [--simulate-failure-after <n>]] [--telemetry-default] [--linger <s>]"
    );
    let models_dir = PathBuf::from(&args[1]);
    let fixtures = PathBuf::from(&args[2]);
    let decoded = PathBuf::from(&args[3]);
    let accel = if args.iter().any(|a| a == "--coreml") {
        Accel::CoreMl { static_shapes: args.iter().any(|a| a == "--static-shapes") }
    } else if args.iter().any(|a| a == "--dml") {
        Accel::Dml
    } else {
        Accel::Cpu
    };
    let flag_value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let telemetry_default = args.iter().any(|a| a == "--telemetry-default");
    if !telemetry_default {
        // ORT reads this while it creates the environment; set it before the library is loaded
        // and before any other thread exists, which is what makes set_var sound here.
        unsafe { std::env::set_var("ORT_DISABLE_TELEMETRY", "1") };
    }
    let mut builder = ort::init_from(&args[4])?;
    if !telemetry_default {
        builder = builder.with_telemetry(false);
    }
    let env = builder.build()?;
    println!("ORT {} loaded, telemetry {}", ort::info(), if telemetry_default { "left at the library default" } else { "disabled" });

    let images = ["astronaut", "messi5", "lena", "car_image", "astronaut-scales", "crowd-280-faces"];
    compare_resize(&decoded, &images)?;

    let t = Instant::now();
    // CoreML rejects the end2end plate graph's empty tensor when nothing is found (ORT 1.30, 1.31),
    // so on CoreML the plate model stays on CPU; YuNet ran on CPU in 1.12.1 except under DML.
    let (yunet_accel, plate_accel) = match accel {
        Accel::CoreMl { .. } if args.iter().any(|a| a == "--plate-coreml") => (Accel::Cpu, accel),
        Accel::CoreMl { .. } => (Accel::Cpu, Accel::Cpu),
        Accel::Dml if args.iter().any(|a| a == "--plate-cpu") => (Accel::Dml, Accel::Cpu),
        other => (other, other),
    };
    let mut models = Models::new(&env, &models_dir, accel, yunet_accel, plate_accel)?;
    println!("sessions ready in {:?} (yolo5face {:?}, yunet {:?}, plate {:?})", t.elapsed(), accel, yunet_accel, plate_accel);

    let mut summary = std::collections::BTreeMap::<&str, Worst>::new();
    for score in [0.1f32, 0.25, 0.5] {
        for name in images {
            let img = Bgr::load_png(&decoded.join(format!("{name}.png")))?;
            for (model, rec) in models.detect(&img, score)? {
                let worst = summary.entry(model).or_insert(Worst { min_iou: 1.0, ..Default::default() });
                compare(&fixtures, model, name, score, &rec, worst)?;
            }
        }
    }
    println!("SUMMARY {}", serde_json::to_string(&summary)?);

    if let Some(seconds) = flag_value("--fallback-test").and_then(|v| v.parse().ok()) {
        let simulate = flag_value("--simulate-failure-after").and_then(|v| v.parse().ok());
        fallback_test(&env, &models_dir, &fixtures, &decoded, &mut models, &images, seconds, simulate)?;
    }

    let img = Bgr::load_png(&decoded.join("astronaut.png"))?;
    let mut pre: Vec<f64> = (0..50)
        .map(|_| {
            let t = Instant::now();
            let canvas = letterbox(&resize_linear_cv(&img, 640, 640), 640, 640, 0, 0, 114);
            std::hint::black_box(blob(&canvas, 1.0 / 255.0, true));
            t.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    pre.sort_by(f64::total_cmp);
    println!("preprocess 512->640 resize+letterbox+blob p50 {:.2} ms", pre[25]);
    for (name, run) in [
        ("yolo5face", &mut (|i: &Bgr| models.yolo.detect(i, 0.5, 0.4).map(drop)) as &mut dyn FnMut(&Bgr) -> Result<()>),
        ("yunet", &mut |i: &Bgr| models.yunet.detect(i, 0.5, 0.4).map(drop)),
        ("plate", &mut |i: &Bgr| models.plate.detect(i, 0.5).map(drop)),
    ] {
        for _ in 0..10 {
            run(&img)?;
        }
        let mut times: Vec<f64> = (0..50)
            .map(|_| {
                let t = Instant::now();
                run(&img).map(|_| t.elapsed().as_secs_f64() * 1000.0)
            })
            .collect::<Result<_>>()?;
        times.sort_by(f64::total_cmp);
        println!("latency {name:9} p50 {:.2} ms p95 {:.2} ms (end to end, 512x512 input)", times[25], times[47]);
    }
    if let Some(seconds) = args.iter().position(|a| a == "--linger").and_then(|i| args.get(i + 1)).and_then(|s| s.parse::<u64>().ok()) {
        // Keeps the environment alive so a background telemetry uploader has time to act.
        println!("lingering {seconds} s with the environment alive");
        std::thread::sleep(std::time::Duration::from_secs(seconds));
    }
    drop(models);
    drop(env);
    Ok(())
}
