//! A video's first pass and tracking through the video processor, with the pinned FFmpeg and a
//! detector that finds the white square the test videos move across a black frame. Needs
//! `CLOAKFRAME_FFMPEG` and `CLOAKFRAME_FFPROBE`.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use cloakframe_core::detection::{Detection, DetectionKind, ModelId, Thresholds};
use cloakframe_core::geometry::Rect;
use cloakframe_core::item::{ItemId, PublishBasis};
use cloakframe_core::review::PhotoEdits;
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::{DetectError, Detector, DetectorHandle, Found, Rgb8Image};
use cloakframe_engine::events::ProgressBoard;
use cloakframe_engine::media::source;
use cloakframe_engine::run::{ItemContext, ItemProcessor, Notes, RenderRequest, StepError};
use cloakframe_engine::video::analysis::AnalysisSettings;
use cloakframe_engine::video::process::{Analyzed, Tools, VideoProcessor, VideoSettings};
use cloakframe_engine::video::record::{Detected, Record, TrackRegion};
use common::{TestDir, permit, required_env};

fn ffmpeg() -> PathBuf {
    required_env("CLOAKFRAME_FFMPEG")
}

fn ffprobe() -> PathBuf {
    required_env("CLOAKFRAME_FFPROBE")
}

fn make(args: &[&str], output: &Path) {
    let status = Command::new(ffmpeg())
        .args(["-v", "error", "-y"])
        .args(args)
        .arg(output)
        .status()
        .unwrap();
    assert!(status.success());
}

/// An 80×80 white square moving 100 pixels a second to the right across 1280×720 black, three
/// seconds at 30 frames per second.
fn moving_square(dir: &TestDir) -> PathBuf {
    let path = dir.path().join("square.mp4");
    make(
        &[
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=1280x720:r=30:d=3[bg];color=c=white:s=80x80:r=30[sq];\
             [bg][sq]overlay=x='100+t*100':y=200:shortest=1",
            "-c:v",
            "libx264",
            "-crf",
            "1",
            "-pix_fmt",
            "yuv420p",
        ],
        &path,
    );
    path
}

/// Where the square is in frame `k`, in display pixels.
fn square(k: u64) -> Rect {
    Rect::new(100.0 + k as f32 * 100.0 / 30.0, 200.0, 80.0, 80.0).unwrap()
}

/// What the detector is asked to do at a frame, counted from 0 by calls.
type Hook = Box<dyn FnMut(u64) + Send>;

/// Finds the white pixels' bounding box as one face, except in `blind` frames. Records each frame's
/// size.
struct WhiteSquare {
    calls: u64,
    blind: std::ops::Range<u64>,
    hook: Option<Hook>,
    sizes: Arc<Mutex<Vec<(u32, u32)>>>,
}

impl WhiteSquare {
    fn new() -> Self {
        Self {
            calls: 0,
            blind: 0..0,
            hook: None,
            sizes: Arc::default(),
        }
    }
}

impl Detector for WhiteSquare {
    fn detect(&mut self, image: &Rgb8Image, _: &Thresholds) -> Result<Found, DetectError> {
        let k = self.calls;
        self.calls += 1;
        if let Some(hook) = &mut self.hook {
            hook(k);
        }
        self.sizes.lock().unwrap().push((image.width, image.height));
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for (i, p) in image.data.as_chunks::<3>().0.iter().enumerate() {
            if p.iter().all(|&v| v > 200) {
                let (x, y) = (i as u32 % image.width, i as u32 / image.width);
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
        if self.blind.contains(&k) || x0 == u32::MAX {
            return Ok(Found::default());
        }
        let bbox = Rect::new(x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32).unwrap();
        let face = Detection::new(ModelId::Yolo5FaceN, 0.9, bbox, None, None).unwrap();
        Ok(Found {
            detections: vec![face],
            ..Found::default()
        })
    }
}

/// Without tiles, so the detector sees each frame once.
fn processor(dir: &TestDir) -> VideoProcessor {
    VideoProcessor::new(
        Tools {
            ffmpeg: ffmpeg(),
            ffprobe: ffprobe(),
        },
        VideoSettings {
            hardware_encode: false,
            masks: vec![Rect::new(0.0, 0.0, 8.0, 8.0).unwrap()],
            analysis: AnalysisSettings {
                tiles: false,
                ..AnalysisSettings::default()
            },
            ..VideoSettings::default()
        },
        dir.path().join("records"),
    )
}

fn with_context<T>(
    input: &Path,
    detector: impl Detector + 'static,
    cancel: &CancelToken,
    step: impl FnOnce(&ItemContext<'_>) -> T,
) -> T {
    let destination = input.parent().unwrap().join("out/clip.mp4");
    let detector = DetectorHandle::spawn(Box::new(detector)).unwrap();
    let notes = Notes::default();
    let ctx = ItemContext {
        item: ItemId(1),
        input,
        destination: &destination,
        cancel,
        detector: &detector,
        progress: ProgressBoard::default().reporter(ItemId(1)),
        notes: &notes,
    };
    step(&ctx)
}

fn render(
    processor: &VideoProcessor,
    ctx: &ItemContext<'_>,
    analysis: &cloakframe_engine::run::Analysis,
    analyzed: Analyzed,
) -> Result<cloakframe_engine::video::process::Rendered, StepError> {
    processor.render(
        ctx,
        &RenderRequest {
            basis: PublishBasis::AutomaticUnreviewed,
            analysis,
            edits: &PhotoEdits::default(),
        },
        Some(analyzed),
    )
}

fn records_left(dir: &TestDir) -> usize {
    match std::fs::read_dir(dir.path().join("records")) {
        Ok(entries) => entries.count(),
        Err(_) => 0,
    }
}

#[test]
fn every_frame_is_detected_and_tracked_into_regions() {
    let dir = TestDir::new();
    let input = moving_square(&dir);
    let processor = processor(&dir);
    let mut detector = WhiteSquare::new();
    // A third of a second missed, which tracking fills.
    detector.blind = 40..50;
    let sizes = detector.sizes.clone();
    let cancel = CancelToken::new();
    with_context(&input, detector, &cancel, |ctx| {
        let (analysis, analyzed) = processor.analyze(ctx).unwrap();
        assert!(analysis.evidence.is_empty(), "{:?}", analysis.evidence);
        assert_eq!(
            analyzed
                .tracks()
                .iter()
                .map(|t| (t.id, t.kind, t.first, t.last))
                .collect::<Vec<_>>(),
            [(1, DetectionKind::Face, 0, 89)]
        );
        let detections = Record::<Detected>::open(&analyzed.records().detections()).unwrap();
        assert_eq!(detections.frames(), 90);

        let regions = Record::<TrackRegion>::open(&analyzed.records().regions()).unwrap();
        let mut frames = regions.ascending().unwrap();
        let mut boxes = Vec::new();
        while let Some(k) = frames.next_into(&mut boxes).unwrap() {
            let [region] = boxes[..] else {
                panic!("frame {k}: {boxes:?}");
            };
            assert_eq!(region.interpolated, (40..50).contains(&k), "frame {k}");
            // Detected at 960×540 and scaled back: within a pixel and a half of the square, the
            // smoothing may only grow it.
            let truth = square(k);
            let grown = Rect::new(
                truth.x() - 1.5,
                truth.y() - 1.5,
                truth.width() + 3.0,
                truth.height() + 3.0,
            )
            .unwrap();
            assert!(
                region.bbox.contains(&grown, 0.0) || truth.iou(&region.bbox) > 0.9,
                "frame {k}: {:?}",
                region.bbox
            );
            assert!(
                region.bbox.area() < truth.area() * 1.25,
                "frame {k}: {:?}",
                region.bbox
            );
        }
        assert_eq!(regions.frames(), 90);

        // Until the second pass masks tracks, a video with one is refused, not published bare.
        let refused = render(&processor, ctx, &analysis, analyzed).err().unwrap();
        assert!(
            refused
                .to_string()
                .contains("cannot be masked in videos yet"),
            "{refused}"
        );
    });
    assert!(sizes.lock().unwrap().iter().all(|&s| s == (960, 540)));
    assert_eq!(records_left(&dir), 0);
}

#[test]
fn cancelling_the_first_pass_stops_it_and_removes_its_records() {
    let dir = TestDir::new();
    let input = moving_square(&dir);
    let processor = processor(&dir);
    let cancel = CancelToken::new();
    let mut detector = WhiteSquare::new();
    let token = cancel.clone();
    detector.hook = Some(Box::new(move |k| {
        if k == 10 {
            token.cancel();
        }
    }));
    let calls = detector.sizes.clone();
    with_context(&input, detector, &cancel, |ctx| {
        assert!(matches!(processor.analyze(ctx), Err(StepError::Cancelled)));
    });
    assert_eq!(calls.lock().unwrap().len(), 11);
    assert_eq!(records_left(&dir), 0);
}

#[test]
fn a_source_that_changes_during_the_first_pass_fails_the_item() {
    let dir = TestDir::new();
    let input = moving_square(&dir);
    let processor = processor(&dir);
    let mut detector = WhiteSquare::new();
    let changed = input.clone();
    detector.hook = Some(Box::new(move |k| {
        if k == 5 {
            std::fs::File::options()
                .write(true)
                .open(&changed)
                .unwrap()
                .set_modified(SystemTime::now() + Duration::from_secs(60))
                .unwrap();
        }
    }));
    let cancel = CancelToken::new();
    with_context(&input, detector, &cancel, |ctx| {
        let e = processor.analyze(ctx).err().unwrap();
        assert_eq!(e.to_string(), "the source changed while it was processed");
    });
    assert_eq!(records_left(&dir), 0);
}

/// FFmpeg's scaler takes each frame to the size the stream started with, so both passes see the
/// same pixels and masks land where they were found (1.12.1 scaled the same way). Where the
/// shape changes, the picture is stretched.
#[test]
fn a_stream_whose_size_changes_is_scaled_to_its_first_size_in_both_passes() {
    let dir = TestDir::new();
    let parts: Vec<PathBuf> = [("320x240", "a"), ("640x360", "b")]
        .iter()
        .map(|(size, name)| {
            let part = dir.path().join(format!("{name}.ts"));
            make(
                &[
                    "-f",
                    "lavfi",
                    "-i",
                    &format!("testsrc2=s={size}:r=30:d=2"),
                    "-c:v",
                    "libx264",
                    "-pix_fmt",
                    "yuv420p",
                ],
                &part,
            );
            part
        })
        .collect();
    let joined = dir.path().join("joined.ts");
    let mut bytes = std::fs::read(&parts[0]).unwrap();
    bytes.extend(std::fs::read(&parts[1]).unwrap());
    std::fs::write(&joined, bytes).unwrap();
    let input = dir.path().join("clip.mp4");
    make(&["-i", joined.to_str().unwrap(), "-c", "copy"], &input);

    let processor = processor(&dir);
    // testsrc2 has white areas; only the sizes matter here.
    let mut detector = WhiteSquare::new();
    detector.blind = 0..u64::MAX;
    let sizes = detector.sizes.clone();
    let cancel = CancelToken::new();
    with_context(&input, detector, &cancel, |ctx| {
        let (analysis, analyzed) = processor.analyze(ctx).unwrap();
        assert!(analyzed.tracks().is_empty());
        let rendered = render(&processor, ctx, &analysis, analyzed).unwrap();
        processor.publish(ctx, rendered, &permit()).unwrap();
    });
    let sizes = sizes.lock().unwrap();
    assert_eq!(sizes.len(), 120);
    assert!(sizes.iter().all(|&s| s == (320, 240)));
    let output = source::open(&ffprobe(), &dir.path().join("out/clip.mp4"), &cancel).unwrap();
    assert_eq!(
        (output.spec.display_size(), output.frames.len()),
        ((320, 240), 120)
    );
}
