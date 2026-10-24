//! A video's first pass, tracking and the second pass that masks what was tracked, through the
//! video processor and the runner, with the pinned FFmpeg and a detector that finds the white
//! square the test videos move across a black frame; and the publish gate of P-1 (video). Needs
//! `CLOAKFRAME_FFMPEG` and `CLOAKFRAME_FFPROBE`.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use cloakframe_core::coverage::{CoverageEvidence, Problem};
use cloakframe_core::detection::{Detection, DetectionKind, ModelId, Thresholds};
use cloakframe_core::geometry::Rect;
use cloakframe_core::item::{ItemId, ItemState, PublishBasis, ReviewMode};
use cloakframe_core::mask::{MaskStyle, Method};
use cloakframe_core::review::{Digest, Edits, Keyframe, ManualTrack, VideoEdits};
use cloakframe_core::tracking::TrackingWindows;
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::{DetectError, Detector, DetectorHandle, Found, Rgb8Image};
use cloakframe_engine::events::ProgressBoard;
use cloakframe_engine::identity::content_digest;
use cloakframe_engine::media::decode::{FrameReader, frame_size};
use cloakframe_engine::media::source;
use cloakframe_engine::run::{
    self, ItemContext, ItemOutcome, ItemProcessor, Job, JobItem, Note, Notes, RenderRequest, Start,
    StepError,
};
use cloakframe_engine::video::analysis::AnalysisSettings;
use cloakframe_engine::video::process::{Analyzed, Tools, VideoProcessor, VideoSettings};
use cloakframe_engine::video::record::{Detected, Record, TrackRegion};
use cloakframe_engine::video::tracking::TrackSummary;
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

/// Finds the white pixels' bounding box as one face, except in `blind` frames, and says it could
/// not return two more in frame `capped`. Records each frame's size.
struct WhiteSquare {
    calls: u64,
    blind: std::ops::Range<u64>,
    capped: Option<u64>,
    hook: Option<Hook>,
    sizes: Arc<Mutex<Vec<(u32, u32)>>>,
}

impl WhiteSquare {
    fn new() -> Self {
        Self {
            calls: 0,
            blind: 0..0,
            capped: None,
            hook: None,
            sizes: Arc::default(),
        }
    }

    fn blind(frames: std::ops::Range<u64>) -> Self {
        Self {
            blind: frames,
            ..Self::new()
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
            over_limit: if self.capped == Some(k) {
                vec![(ModelId::Yolo5FaceN, 2)]
            } else {
                Vec::new()
            },
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
            edits: &Edits::default(),
        },
        Some(analyzed),
    )
}

fn tracks(analysis: &cloakframe_engine::run::Analysis) -> &[TrackSummary] {
    &analysis.video.as_ref().expect("a video's analysis").tracks
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
            tracks(&analysis)
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

        let rendered = render(&processor, ctx, &analysis, analyzed).unwrap();
        processor.publish(ctx, rendered, &permit()).unwrap();
    });
    assert!(sizes.lock().unwrap().iter().all(|&s| s == (960, 540)));
    assert_eq!(records_left(&dir), 0);
    let output = output_frames(&dir.path().join("out/clip.mp4"));
    assert_eq!(output.len(), 90);
    for (k, frame) in output.iter().enumerate() {
        assert!(masked(frame, k as u64), "frame {k}");
    }
}

/// Every frame of a published output at its display size.
fn output_frames(path: &Path) -> Vec<Rgb8Image> {
    let cancel = CancelToken::new();
    let source = source::open(&ffprobe(), path, &cancel).unwrap();
    let size = frame_size(&source.spec, None);
    let mut reader =
        FrameReader::start(&ffmpeg(), path, &source.spec, &source.frames, size, &cancel).unwrap();
    let mut frames = Vec::new();
    loop {
        let mut frame = Rgb8Image {
            width: 0,
            height: 0,
            data: Vec::new(),
        };
        if !reader.next_into(&mut frame).unwrap() {
            return frames;
        }
        frames.push(frame);
    }
}

/// The square's pixels in frame `k`, inset from its edges.
fn square_pixels(frame: &Rgb8Image, k: u64) -> impl Iterator<Item = [u8; 3]> + '_ {
    let s = square(k);
    let (x0, y0) = (s.x() as u32 + 8, s.y() as u32 + 8);
    (y0..y0 + 64).step_by(8).flat_map(move |y| {
        (x0..x0 + 64).step_by(8).map(move |x| {
            let i = (y as usize * frame.width as usize + x as usize) * 3;
            [frame.data[i], frame.data[i + 1], frame.data[i + 2]]
        })
    })
}

/// The square is under the default fill's black, none of its white left.
fn masked(frame: &Rgb8Image, k: u64) -> bool {
    square_pixels(frame, k).all(|p| p.iter().all(|&c| c <= 16))
}

/// The square shows, white as in the source.
fn bare(frame: &Rgb8Image, k: u64) -> bool {
    square_pixels(frame, k).all(|p| p.iter().all(|&c| c >= 230))
}

/// `input` through the runner in automatic mode, published as `out/clip.mp4`.
fn run_one(processor: VideoProcessor, input: &Path, detector: WhiteSquare) -> ItemOutcome {
    let job = Job {
        items: vec![JobItem {
            id: ItemId(1),
            input: input.to_path_buf(),
            destination: input.parent().unwrap().join("out/clip.mp4"),
            start: Start::Fresh {
                needs_review: false,
            },
        }],
        review: ReviewMode::Off,
        workers: 1,
    };
    let detector = DetectorHandle::spawn(Box::new(detector)).unwrap();
    let report = run::start(job, Arc::new(processor), detector)
        .unwrap()
        .wait();
    report.items.into_iter().next().unwrap()
}

/// The windows of the default but holes filled for only a second, shorter than the two the
/// tracker bridges: the defaults leave no gap (docs/M5_RENDER.md), these do.
fn short_fill() -> TrackingWindows {
    TrackingWindows {
        max_interpolation: 1.0,
        ..TrackingWindows::default()
    }
}

fn processor_with(dir: &TestDir, windows: TrackingWindows, masks: Vec<Rect>) -> VideoProcessor {
    VideoProcessor::new(
        Tools {
            ffmpeg: ffmpeg(),
            ffprobe: ffprobe(),
        },
        VideoSettings {
            hardware_encode: false,
            masks,
            analysis: AnalysisSettings {
                tiles: false,
                windows,
                ..AnalysisSettings::default()
            },
            ..VideoSettings::default()
        },
        dir.path().join("records"),
    )
}

fn nothing_published(dir: &TestDir) -> bool {
    let out = dir.path().join("out");
    !out.exists() || std::fs::read_dir(out).unwrap().next().is_none()
}

/// P-1 (video, tests/fixtures/defects/P-1/expected.json): the face is lost in frames 30–64 and
/// found again. A hole tracking bridges but does not fill is an unresolved gap: the video waits
/// for review in automatic mode with its records, and nothing is written to the output folder.
#[test]
fn an_unfilled_gap_holds_the_video_for_review_and_publishes_nothing() {
    let dir = TestDir::new();
    let input = moving_square(&dir);
    let outcome = run_one(
        processor_with(&dir, short_fill(), Vec::new()),
        &input,
        WhiteSquare::blind(30..65),
    );
    assert_eq!(
        outcome.state,
        ItemState::NeedsReview,
        "{:?}",
        outcome.failure
    );
    assert_eq!(
        outcome.analysis.evidence,
        [CoverageEvidence::unresolved(Problem::UnresolvedGap {
            detection: DetectionKind::Face,
            tracks: vec![1],
            first: 30,
            last: 64,
        })]
    );
    assert!(nothing_published(&dir));
    assert!(dir.path().join("records/item-1/regions").exists());
}

/// Detections a bounded model could not return hold a video as they hold a photo (P-1).
#[test]
fn capped_candidates_hold_the_video_for_review() {
    let dir = TestDir::new();
    let input = moving_square(&dir);
    let mut detector = WhiteSquare::new();
    detector.capped = Some(10);
    let outcome = run_one(
        processor_with(&dir, TrackingWindows::default(), Vec::new()),
        &input,
        detector,
    );
    assert_eq!(
        outcome.state,
        ItemState::NeedsReview,
        "{:?}",
        outcome.failure
    );
    assert_eq!(
        outcome.analysis.evidence,
        [CoverageEvidence::unresolved(Problem::CappedCandidates {
            model: ModelId::Yolo5FaceN,
            frame: Some(10),
            kept: 1,
            found: 3,
        })]
    );
    assert!(nothing_published(&dir));
}

/// With the default windows the same miss is bridged and filled: the video is published with
/// the square masked in every frame, the missed ones too (docs/M5_RENDER.md). 1.12.1 reported
/// such a hole and published it bare.
#[test]
fn the_default_windows_mask_a_subject_missed_for_over_a_second() {
    let dir = TestDir::new();
    let input = moving_square(&dir);
    let outcome = run_one(
        processor_with(&dir, TrackingWindows::default(), Vec::new()),
        &input,
        WhiteSquare::blind(30..65),
    );
    assert_eq!(
        outcome.state,
        ItemState::Published(PublishBasis::AutomaticUnreviewed),
        "{:?}",
        outcome.failure
    );
    assert!(outcome.analysis.evidence.is_empty());
    assert_eq!(
        outcome.notes,
        [Note::Tracked {
            faces: 1,
            plates: 0
        }]
    );
    let output = output_frames(&dir.path().join("out/clip.mp4"));
    assert_eq!(output.len(), 90);
    for (k, frame) in output.iter().enumerate() {
        assert!(masked(frame, k as u64), "frame {k}");
    }
}

/// The reviewer's two ways past an unresolved gap (plan 4.2), at the processor: exporting with
/// warnings masks what was tracked and leaves the gap as it is; exporting without anonymization
/// masks nothing, not even the fixed areas. Resuming a video from a stored review is M5 stage 3.
#[test]
fn exports_past_a_gap_mask_what_was_tracked_or_nothing() {
    for basis in [
        PublishBasis::ExportedWithWarnings,
        PublishBasis::ExportedWithoutAnonymization,
    ] {
        let dir = TestDir::new();
        let input = moving_square(&dir);
        let fixed = Rect::new(100.0, 200.0, 80.0, 80.0).unwrap();
        let processor = processor_with(&dir, short_fill(), vec![fixed]);
        let cancel = CancelToken::new();
        with_context(&input, WhiteSquare::blind(30..65), &cancel, |ctx| {
            let (analysis, analyzed) = processor.analyze(ctx).unwrap();
            assert_eq!(analysis.evidence.len(), 1);
            let rendered = processor
                .render(
                    ctx,
                    &RenderRequest {
                        basis,
                        analysis: &analysis,
                        edits: &Edits::default(),
                    },
                    Some(analyzed),
                )
                .unwrap();
            processor.publish(ctx, rendered, &permit()).unwrap();
        });
        let output = output_frames(&dir.path().join("out/clip.mp4"));
        assert_eq!(output.len(), 90);
        for (k, frame) in output.iter().enumerate() {
            let k = k as u64;
            if basis == PublishBasis::ExportedWithoutAnonymization {
                assert!(bare(frame, k), "{basis:?} frame {k}");
            } else if !(30..=64).contains(&k) {
                assert!(masked(frame, k), "{basis:?} frame {k}");
            } else if (33..62).contains(&k) {
                // Past the track's ends held over for a tenth of a second.
                assert!(bare(frame, k), "{basis:?} frame {k}");
            }
        }
    }
}

/// A frame of a still picture repeated for two seconds.
fn still_picture(dir: &TestDir) -> PathBuf {
    let path = dir.path().join("still.mp4");
    make(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=320x240:r=30:d=2,trim=end_frame=1,loop=loop=59:size=1:start=0",
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

/// The same box a few pixels off in each frame, as a detector gives a face that keeps still.
struct Jittering {
    calls: usize,
}

impl Detector for Jittering {
    fn detect(&mut self, _: &Rgb8Image, _: &Thresholds) -> Result<Found, DetectError> {
        const OFFSETS: [(f32, f32, f32); 5] = [
            (0.0, 0.0, 0.0),
            (2.5, -1.5, 3.0),
            (-2.0, 2.0, -2.0),
            (1.0, 3.0, 1.5),
            (-3.0, -2.5, -3.0),
        ];
        let (dx, dy, dw) = OFFSETS[self.calls % OFFSETS.len()];
        self.calls += 1;
        let bbox = Rect::new(120.0 + dx, 80.0 + dy, 60.0 + dw, 70.0 + dw).unwrap();
        Ok(Found {
            detections: vec![Detection::new(ModelId::Yolo5FaceN, 0.9, bbox, None, None).unwrap()],
            ..Found::default()
        })
    }
}

/// A track's mosaic cells keep their size and place (plan 4.4): over a still picture, every
/// frame of a jittering box shows the same mosaic, where cells fitted to each frame's box would
/// change from frame to frame (docs/M5_RENDER.md).
#[test]
fn a_tracks_mosaic_keeps_its_cells_while_its_box_jitters() {
    let dir = TestDir::new();
    let input = still_picture(&dir);
    let processor = VideoProcessor::new(
        Tools {
            ffmpeg: ffmpeg(),
            ffprobe: ffprobe(),
        },
        VideoSettings {
            hardware_encode: false,
            style: MaskStyle {
                method: Method::Mosaic,
                ..MaskStyle::default()
            },
            analysis: AnalysisSettings {
                tiles: false,
                ..AnalysisSettings::default()
            },
            ..VideoSettings::default()
        },
        dir.path().join("records"),
    );
    let cancel = CancelToken::new();
    with_context(&input, Jittering { calls: 0 }, &cancel, |ctx| {
        let (analysis, analyzed) = processor.analyze(ctx).unwrap();
        assert_eq!(tracks(&analysis).len(), 1);
        let rendered = render(&processor, ctx, &analysis, analyzed).unwrap();
        processor.publish(ctx, rendered, &permit()).unwrap();
    });
    let output = output_frames(&dir.path().join("out/clip.mp4"));
    assert_eq!(output.len(), 60);
    // Inside every frame's box: the boxes vary by up to 3 pixels around (120, 80, 60, 70).
    let at = |f: &Rgb8Image, x: u32, y: u32| {
        let i = (y as usize * f.width as usize + x as usize) * 3;
        [f.data[i], f.data[i + 1], f.data[i + 2]]
    };
    let mut largest = 0;
    for frame in &output[1..] {
        for y in 85..145 {
            for x in 125..175 {
                let (a, b) = (at(&output[0], x, y), at(frame, x, y));
                let d = a.iter().zip(&b).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
                largest = largest.max(d);
            }
        }
    }
    // A lossy encode of the same flat cells.
    assert!(largest <= 12, "{largest}");
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
        assert!(tracks(&analysis).is_empty());
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

/// A video analyzed and left for review, as the runner leaves it: its records kept, the
/// prepared state dropped.
fn analyzed_for_review(dir: &TestDir) -> (PathBuf, VideoProcessor, run::Analysis) {
    let input = moving_square(dir);
    let processor = processor_with(dir, TrackingWindows::default(), Vec::new());
    let analysis = with_context(&input, WhiteSquare::new(), &CancelToken::new(), |ctx| {
        let (analysis, analyzed) = processor.analyze(ctx).unwrap();
        drop(analyzed);
        analysis
    });
    assert!(dir.path().join("records/item-1/regions").exists());
    (input, processor, analysis)
}

/// The square's track, drawn by hand: it moves in a straight line, so two keyframes follow it.
fn by_hand() -> ManualTrack {
    ManualTrack {
        first: 0,
        last: 89,
        keyframes: vec![
            Keyframe {
                frame: 0,
                bbox: square(0),
            },
            Keyframe {
                frame: 89,
                bbox: square(89),
            },
        ],
    }
}

fn excluding(added: Vec<ManualTrack>) -> VideoEdits {
    VideoEdits {
        excluded_tracks: vec![1],
        added_tracks: added,
    }
}

fn resume(
    processor: &VideoProcessor,
    input: &Path,
    analysis: &run::Analysis,
    edits: VideoEdits,
) -> Result<(), StepError> {
    let edits = Edits {
        video: edits,
        ..Edits::default()
    };
    with_context(input, WhiteSquare::new(), &CancelToken::new(), |ctx| {
        let rendered = processor.render(
            ctx,
            &RenderRequest {
                basis: PublishBasis::Reviewed,
                analysis,
                edits: &edits,
            },
            None,
        )?;
        processor.publish(ctx, rendered, &permit())
    })
}

/// A review resumed from its stored analysis renders the same bytes on the same timeline again,
/// with the reviewer's edits: an excluded track is left bare, a track drawn by hand is masked.
/// Its records go once it is published.
#[test]
fn a_stored_analysis_renders_again_with_the_reviewers_edits() {
    for added in [Vec::new(), vec![by_hand()]] {
        let dir = TestDir::new();
        let (input, processor, analysis) = analyzed_for_review(&dir);
        assert_eq!(
            analysis.input,
            Some(content_digest(&std::fs::read(&input).unwrap()))
        );
        let drawn = !added.is_empty();
        resume(&processor, &input, &analysis, excluding(added)).unwrap();
        assert!(!dir.path().join("records/item-1").exists());
        let output = output_frames(&dir.path().join("out/clip.mp4"));
        assert_eq!(output.len(), 90);
        for (k, frame) in output.iter().enumerate() {
            let k = k as u64;
            assert!(
                if drawn {
                    masked(frame, k)
                } else {
                    bare(frame, k)
                },
                "frame {k}"
            );
        }
    }
}

/// Other bytes, or the same bytes read as another timeline, are not rendered from a review.
#[test]
fn a_review_is_rendered_only_from_the_analyzed_bytes_and_timeline() {
    let dir = TestDir::new();
    let (input, processor, analysis) = analyzed_for_review(&dir);
    let mut other_bytes = analysis.clone();
    other_bytes.input = Some(Digest([0; 32]));
    let mut other_timeline = analysis.clone();
    other_timeline.video.as_mut().unwrap().timeline = Digest([0; 32]);
    for changed in [other_bytes, other_timeline] {
        let Err(StepError::Failed(e)) = resume(&processor, &input, &changed, VideoEdits::default())
        else {
            panic!("rendered a changed source");
        };
        assert!(
            e.to_string().contains("changed since it was analyzed"),
            "{e}"
        );
        assert!(nothing_published(&dir));
    }
    assert!(dir.path().join("records/item-1/regions").exists());
    resume(&processor, &input, &analysis, VideoEdits::default()).unwrap();
}
