//! The 1.12.1 tracking tests (tests/fixtures/legacy/tracking_tests). Normative tests replay
//! every recorded call and compare the outputs; intent tests check what the 1.12.1 test asserted,
//! not its recorded values (index.json).

use std::path::PathBuf;

use cloakframe_core::geometry::Rect;
use cloakframe_core::tracking::merge::{build_bidirectional_tracks, build_tracks};
use cloakframe_core::tracking::post::{
    UncoveredSpan, extend_track_ends, interpolate_gaps, post_process_tracks, smooth_track,
};
use cloakframe_core::tracking::scene::{SceneCutDetector, SceneCuts, Thumbnail};
use cloakframe_core::tracking::{
    Observation, PostProcessConfig, Track, TrackedBox, TrackerConfig, regions_for_frame,
};
use serde_json::Value;

fn legacy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/legacy")
}

fn read(path: PathBuf) -> Value {
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap()
}

fn f(v: &Value) -> f32 {
    v.as_f64().unwrap() as f32
}

fn u(v: &Value) -> u64 {
    v.as_u64().unwrap()
}

fn rect(v: &Value) -> Rect {
    Rect::new(f(&v[0]), f(&v[1]), f(&v[2]), f(&v[3])).unwrap()
}

/// 1.12.1's `isValidFacePose`.
fn roll(v: &Value) -> Option<f32> {
    let r = f(&v["roll"]);
    (v["has_pose"].as_bool().unwrap() && r.is_finite() && r.abs() <= std::f32::consts::FRAC_PI_3)
        .then_some(r)
}

fn observation(v: &Value) -> Observation {
    Observation {
        bbox: rect(&v["box"]),
        score: f(&v["score"]),
        roll: roll(v),
    }
}

fn frames(v: &Value) -> Vec<Vec<Observation>> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_array().unwrap().iter().map(observation).collect())
        .collect()
}

fn tracked(v: &Value) -> TrackedBox {
    TrackedBox {
        frame: u(&v["frame"]),
        bbox: rect(&v["box"]),
        score: f(&v["score"]),
        interpolated: v["interpolated"].as_bool().unwrap(),
        roll: roll(v),
    }
}

fn track(v: &Value) -> Track {
    Track {
        id: u(&v["id"]) as u32,
        low_confidence: v["low_confidence"].as_bool().unwrap(),
        boxes: v["boxes"].as_array().unwrap().iter().map(tracked).collect(),
    }
}

fn tracks(v: &Value) -> Vec<Track> {
    v.as_array().unwrap().iter().map(track).collect()
}

fn cuts(v: &Value) -> SceneCuts {
    SceneCuts::new(v.as_array().unwrap().iter().map(u))
}

fn spans(v: &Value) -> Vec<UncoveredSpan> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| UncoveredSpan {
            first: u(&s[0]),
            last: u(&s[1]),
        })
        .collect()
}

fn tracker_config(v: &Value) -> TrackerConfig {
    TrackerConfig {
        high_score: f(&v["high_score_threshold"]),
        low_score: f(&v["low_score_threshold"]),
        iou: f(&v["iou_threshold"]),
        max_frames_lost: u(&v["max_frames_lost"]),
        max_frames_since_high: u(&v["max_frames_since_high_score"]),
        max_frames_since_high_moving: u(&v["max_frames_since_high_score_moving"]),
        coast_motion: f(&v["coast_motion_threshold"]),
        velocity_blend: f(&v["velocity_blend"]),
    }
}

fn post_config(v: &Value) -> PostProcessConfig {
    PostProcessConfig {
        max_interpolation_gap: u(&v["max_interpolation_gap"]),
        smoothing_radius: u(&v["smoothing_radius"]),
        extension_frames: u(&v["extension_frames"]),
        max_cut_boundary_gap: u(&v["max_cut_boundary_gap"]),
        strong_score: f(&v["strong_score_threshold"]),
        min_strong: u(&v["min_strong_detections"]) as u32,
        short_track_min_strong: u(&v["short_track_min_strong"]) as u32,
        short_track_strong_ratio: f(&v["short_track_strong_ratio"]),
        retain_without_strong: v["retain_tracks_without_strong_detection"]
            .as_bool()
            .unwrap(),
    }
}

fn thumbnail(v: &Value) -> Thumbnail {
    let (w, h) = (u(&v["width"]) as usize, u(&v["height"]) as usize);
    let pixels: Vec<u8> = (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| match v["kind"].as_str().unwrap() {
                "solid" => u(&v["value"]) as u8,
                "gradient_x" => (x * 255 / (w - 1)) as u8,
                "gradient_y" => (y * 255 / (h - 1)) as u8,
                kind => panic!("frame kind {kind}"),
            })
        })
        .collect();
    Thumbnail::from_grey(w, h, &pixels)
}

fn close(a: f32, b: f32, tolerance: f32) -> bool {
    (a - b).abs() <= tolerance * (1.0 + b.abs())
}

fn assert_box(got: &TrackedBox, want: &TrackedBox, context: &str) {
    let (g, w) = (got.bbox, want.bbox);
    let same = got.frame == want.frame
        && got.interpolated == want.interpolated
        && close(got.score, want.score, 1e-6)
        && [
            (g.x(), w.x()),
            (g.y(), w.y()),
            (g.width(), w.width()),
            (g.height(), w.height()),
        ]
        .iter()
        .all(|&(a, b)| close(a, b, 1e-4))
        && match (got.roll, want.roll) {
            (None, None) => true,
            (Some(a), Some(b)) => close(a, b, 1e-4),
            _ => false,
        };
    assert!(same, "{context}: {got:?} != {want:?}");
}

fn assert_track(got: &Track, want: &Track, context: &str) {
    assert_eq!(got.id, want.id, "{context}: id");
    assert_eq!(
        got.low_confidence, want.low_confidence,
        "{context}: low confidence"
    );
    assert_eq!(
        got.boxes.iter().map(|b| b.frame).collect::<Vec<_>>(),
        want.boxes.iter().map(|b| b.frame).collect::<Vec<_>>(),
        "{context}: frames of track {}",
        want.id
    );
    for (g, w) in got.boxes.iter().zip(&want.boxes) {
        assert_box(g, w, context);
    }
}

fn assert_tracks(got: &[Track], want: &[Track], context: &str) {
    assert_eq!(got.len(), want.len(), "{context}: track count");
    for (g, w) in got.iter().zip(want) {
        assert_track(g, w, context);
    }
}

/// Replays one recorded call and compares its outputs.
fn replay(call: &Value, context: &str) {
    let context = &format!("{context} {}", call["fn"]);
    match call["fn"].as_str().unwrap() {
        "buildTracks" => {
            let got = build_tracks(
                &frames(&call["frames"]),
                &tracker_config(&call["config"]),
                &cuts(&call["scene_cuts"]),
            );
            assert_tracks(&got, &tracks(&call["tracks"]), context);
        }
        "buildBidirectionalTracks" => {
            let got = build_bidirectional_tracks(
                &frames(&call["frames"]),
                &tracker_config(&call["config"]),
                f(&call["merge_iou_threshold"]),
                &cuts(&call["scene_cuts"]),
            );
            assert_tracks(&got, &tracks(&call["tracks"]), context);
        }
        "interpolateGaps" => {
            let mut t = track(&call["track_before"]);
            let got = interpolate_gaps(&mut t, u(&call["max_gap"]), &cuts(&call["scene_cuts"]));
            assert_track(&t, &track(&call["track_after"]), context);
            assert_eq!(got, spans(&call["uncovered_spans"]), "{context}");
        }
        "smoothTrack" => {
            let mut t = track(&call["track_before"]);
            smooth_track(&mut t, u(&call["radius"]));
            assert_track(&t, &track(&call["track_after"]), context);
        }
        "extendTrackEnds" => {
            let mut t = track(&call["track_before"]);
            extend_track_ends(
                &mut t,
                u(&call["frames"]),
                u(&call["frame_count"]),
                &cuts(&call["scene_cuts"]),
            );
            assert_track(&t, &track(&call["track_after"]), context);
        }
        "postProcessTracks" => {
            let mut t = tracks(&call["tracks_before"]);
            let report = post_process_tracks(
                &mut t,
                &post_config(&call["config"]),
                u(&call["frame_count"]),
                &cuts(&call["scene_cuts"]),
            );
            assert_tracks(&t, &tracks(&call["tracks_after"]), context);
            let want = &call["report"];
            assert_eq!(
                report.uncovered_spans,
                spans(&want["uncovered_spans"]),
                "{context}"
            );
            assert_eq!(
                report.uncovered_frames,
                u(&want["uncovered_frames"]),
                "{context}"
            );
            assert_eq!(
                u64::from(report.dropped_tracks),
                u(&want["dropped_tracks"]),
                "{context}"
            );
        }
        "SceneCutDetector" => {
            let mut detector = SceneCutDetector::default();
            for push in call["pushed"].as_array().unwrap() {
                let t = thumbnail(&push["frame"]);
                for _ in 0..u(&push["repeat"]) {
                    detector.push(t.clone());
                }
            }
            assert_eq!(detector.finish(), cuts(&call["cuts"]), "{context}");
        }
        other => panic!("{context}: no replay for {other}"),
    }
}

/// Normative tests whose recorded calls are replayed. The other normative tests are checked
/// elsewhere: the frame-rate windows in `tracking::tests` (the interpolation window changed, M5),
/// the manual track in M5 stage 3 with video review, the scene-cut container in
/// `tracking::scene::tests`.
const REPLAYED: &[&str] = &[
    "testBidirectionalTracksRespectConsecutiveSceneCuts",
    "testBidirectionalTracksRespectSceneCuts",
    "testConstantVelocitySingleTrack",
    "testCoveredCutGapStaysWithEachSideOfTheCut",
    "testExtendTrackEndsClampsToVideoBounds",
    "testExtendTrackEndsStopsAtSceneCut",
    "testGapAcrossSceneCutIsBoundedInTime",
    "testGapAcrossSceneCutIsNotReportedForAnotherPlace",
    "testGapInterpolationFillsMissedFrames",
    "testGapLargerThanLimitIsReportedAsUncovered",
    "testGapsSharedByTracksAreReportedOnce",
    "testGuardRejectedGapIsCoveredConservatively",
    "testInterpolationCoversSizeJumpConservatively",
    "testLongGapAcrossSceneCutIsReportedWhenTracksMeet",
    "testNoInterpolationAcrossSceneCut",
    "testPoseIsTrackedAndInterpolated",
    "testRegionsForFrameCollectsAllTracks",
    "testSceneCutDetectorCommitsCandidateRaisedWhileConfirming",
    "testSceneCutDetectorCommitsTrailingCandidate",
    "testSceneCutDetectorFindsBothCutsAroundShortShot",
    "testSceneCutDetectorFindsHardCut",
    "testSceneCutDetectorIgnoresFlash",
    "testSceneCutDetectorIgnoresStaticScene",
    "testShortGapAcrossSceneCutIsCoveredWhenTracksMeet",
    "testSmoothingNeverUncoversDetections",
    "testTrackerSplitsTracksAtSceneCut",
];

#[test]
fn normative_tests_give_the_recorded_results() {
    let index = read(legacy().join("index.json"));
    for name in REPLAYED {
        let status = &index["tests"][format!("tracking_tests/{name}")]["status"];
        assert_eq!(status, "normative", "{name}");
        let test = read(legacy().join(format!("tracking_tests/{name}.json")));
        let calls = test["calls"].as_array().unwrap();
        assert!(!calls.is_empty(), "{name} has calls");
        for call in calls {
            replay(call, name);
        }
    }
}

/// Every tracking test of 1.12.1 is replayed here, checked by hand below or in a unit test, or
/// left to a later stage with a reason.
#[test]
fn every_legacy_tracking_test_is_accounted_for() {
    let index = read(legacy().join("index.json"));
    let elsewhere = [
        "testTrackingWindowsFollowTheFrameRate",
        "testSceneCutsContainer",
        "testManualVideoTrackInterpolationAndBounds",
        "testSceneCutDetectorReportsSingleFrameShotAsOneBoundary",
        "testTrackingSafetyLimitsRejectExcessiveData",
        "testTrackingCancellationGuard",
    ];
    let source = include_str!("tracking_fixtures.rs");
    for (key, _) in index["tests"].as_object().unwrap() {
        let Some(name) = key.strip_prefix("tracking_tests/") else {
            continue;
        };
        let hand_checked = source.contains(&format!("/// {name}"));
        assert!(
            REPLAYED.contains(&name) || elsewhere.contains(&name) || hand_checked,
            "{name} is not checked"
        );
    }
}

fn det(x: f32, y: f32, score: f32, size: f32) -> Observation {
    Observation {
        bbox: Rect::new(x, y, size, size).unwrap(),
        score,
        roll: None,
    }
}

fn moving(frames: usize, start_x: f32, step_x: f32, y: f32, score: f32) -> Vec<Vec<Observation>> {
    (0..frames)
        .map(|k| vec![det(start_x + step_x * k as f32, y, score, 40.0)])
        .collect()
}

fn config() -> TrackerConfig {
    TrackerConfig::default()
}

fn no_cuts() -> SceneCuts {
    SceneCuts::default()
}

fn bidirectional(frames: &[Vec<Observation>], cuts: &SceneCuts) -> Vec<Track> {
    build_bidirectional_tracks(frames, &config(), 0.5, cuts)
}

/// 1.12.1's post-processing defaults; the interpolation window of the fixtures is 20.
fn legacy_post() -> PostProcessConfig {
    PostProcessConfig {
        max_interpolation_gap: 20,
        ..PostProcessConfig::default()
    }
}

/// testLowConfidenceDetectionsExtendButNeverStartTracks
#[test]
fn weak_detections_extend_but_never_start_tracks() {
    let mut sequence = moving(15, 50.0, 5.0, 100.0, 0.9);
    for frame in &mut sequence[5..=7] {
        frame[0].score = 0.2;
    }
    let tracks = build_tracks(&sequence, &config(), &no_cuts());
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].boxes.len(), 15);
    assert!(tracks[0].box_at(6).is_some());
    assert!(build_tracks(&moving(15, 50.0, 5.0, 100.0, 0.2), &config(), &no_cuts()).is_empty());
}

/// testLowConfidenceCoastingExpires
#[test]
fn coasting_on_weak_detections_ends() {
    let mut sequence = moving(60, 50.0, 0.0, 100.0, 0.2);
    for frame in &mut sequence[..=4] {
        frame[0].score = 0.9;
    }
    let tracks = build_tracks(&sequence, &config(), &no_cuts());
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].last_frame(), Some(34));
}

/// testMovingCoastingSurvivesLongerThanStatic
#[test]
fn a_moving_track_coasts_longer_than_a_still_one() {
    for (step, last) in [(0.0, 34), (6.0, 49)] {
        let mut sequence = moving(60, 50.0, step, 100.0, 0.2);
        for frame in &mut sequence[..=4] {
            frame[0].score = 0.9;
        }
        let tracks = build_tracks(&sequence, &config(), &no_cuts());
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].last_frame(), Some(last));
    }
}

/// testAConfirmedWeakTrackIsMaskedWhole
#[test]
fn a_confirmed_weak_track_is_masked_whole() {
    let mut weak = moving(20, 50.0, 5.0, 100.0, 0.2);
    weak[3][0].score = 0.9;
    weak[4][0].score = 0.9;
    let mut tracks = bidirectional(&weak, &no_cuts());
    assert_eq!(tracks.len(), 1);
    let no_extension = PostProcessConfig {
        extension_frames: 0,
        ..legacy_post()
    };
    let report = post_process_tracks(&mut tracks, &no_extension, 20, &no_cuts());
    assert_eq!(tracks.len(), 1);
    assert!(tracks[0].low_confidence);
    assert!((0..20).all(|f| tracks[0].box_at(f).is_some()));
    assert_eq!((report.dropped_tracks, report.uncovered_frames), (0, 0));

    let mut nothing_strong = bidirectional(&weak, &no_cuts());
    let strict = PostProcessConfig {
        strong_score: 0.95,
        ..legacy_post()
    };
    let report = post_process_tracks(&mut nothing_strong, &strict, 20, &no_cuts());
    assert!(nothing_strong.is_empty());
    assert_eq!(report.dropped_tracks, 1);

    let mut solid = bidirectional(&moving(20, 50.0, 5.0, 100.0, 0.9), &no_cuts());
    assert_eq!(solid.len(), 1);
    post_process_tracks(&mut solid, &legacy_post(), 20, &no_cuts());
    assert_eq!(solid.len(), 1);
}

/// testRetainedLowConfidenceTracksAreMarkedNotDropped
#[test]
fn kept_weak_tracks_are_marked_low_confidence() {
    let mut weak = moving(20, 50.0, 5.0, 100.0, 0.2);
    weak[3][0].score = 0.9;
    weak[4][0].score = 0.9;
    let mut tracks = bidirectional(&weak, &no_cuts());
    tracks.extend(bidirectional(
        &moving(20, 300.0, 5.0, 300.0, 0.9),
        &no_cuts(),
    ));
    assert_eq!(tracks.len(), 2);
    let retain = PostProcessConfig {
        retain_without_strong: true,
        ..legacy_post()
    };
    post_process_tracks(&mut tracks, &retain, 20, &no_cuts());
    assert_eq!(tracks.len(), 2);
    let low: Vec<&Track> = tracks.iter().filter(|t| t.low_confidence).collect();
    assert_eq!(low.len(), 1);
    assert!(low[0].box_at(3).is_some() && low[0].box_at(4).is_some());
}

/// testShortStrongBurstIsKept
#[test]
fn a_short_strong_burst_is_kept() {
    let mut tracks = bidirectional(&moving(2, 50.0, 5.0, 100.0, 0.9), &no_cuts());
    assert_eq!(tracks.len(), 1);
    post_process_tracks(&mut tracks, &legacy_post(), 2, &no_cuts());
    assert_eq!(tracks.len(), 1);
    assert!(!tracks[0].low_confidence);
}

/// testHighConfidenceSingletonIsKept
#[test]
fn a_confident_single_detection_is_kept() {
    let mut sequence = vec![Vec::new(); 6];
    sequence[5].push(det(50.0, 100.0, 0.9, 40.0));
    let cuts = SceneCuts::new([5]);
    let mut tracks = bidirectional(&sequence, &cuts);
    assert_eq!(tracks.len(), 1);
    post_process_tracks(&mut tracks, &legacy_post(), 6, &cuts);
    assert_eq!(tracks.len(), 1);
    assert!(tracks[0].box_at(5).is_some());

    let mut strict_tracks = bidirectional(&sequence, &cuts);
    let strict = PostProcessConfig {
        short_track_min_strong: 2,
        ..legacy_post()
    };
    let report = post_process_tracks(&mut strict_tracks, &strict, 6, &cuts);
    assert_eq!(strict_tracks.len(), 1);
    assert!(strict_tracks[0].low_confidence);
    assert!(strict_tracks[0].box_at(5).is_some());
    assert_eq!(report.dropped_tracks, 0);
}

/// testCrossingObjectsKeepTwoTracks
#[test]
fn crossing_subjects_keep_two_tracks() {
    let sequence: Vec<Vec<Observation>> = (0..=20)
        .map(|k| {
            let offset = 10.0 * k as f32;
            vec![
                det(offset, 100.0, 0.9, 40.0),
                det(200.0 - offset, 100.0, 0.9, 40.0),
            ]
        })
        .collect();
    let tracks = build_tracks(&sequence, &config(), &no_cuts());
    assert_eq!(tracks.len(), 2);
    for frame in 0..=20 {
        assert_eq!(regions_for_frame(&tracks, frame).len(), 2);
    }
    // After crossing, each track keeps moving the way it was: the first goes right.
    let first = &tracks[0];
    assert!(first.box_at(20).unwrap().bbox.x() > first.box_at(0).unwrap().bbox.x());
}

/// testBidirectionalMergeProducesSingleTrack
#[test]
fn both_passes_over_one_subject_give_one_track() {
    let tracks = bidirectional(&moving(20, 50.0, 5.0, 100.0, 0.9), &no_cuts());
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].boxes.len(), 20);
}

/// testBackwardPassRecoversLowConfidenceStart
#[test]
fn the_backward_pass_recovers_a_weak_start() {
    let mut sequence = moving(20, 50.0, 5.0, 100.0, 0.9);
    for frame in &mut sequence[..=4] {
        frame[0].score = 0.2;
    }
    let forward = build_tracks(&sequence, &config(), &no_cuts());
    assert_eq!(forward.len(), 1);
    assert_eq!(forward[0].first_frame(), Some(5));
    let merged = bidirectional(&sequence, &no_cuts());
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].first_frame(), Some(0));
    assert_eq!(merged[0].boxes.len(), 20);
}

/// testPoseSmoothingReducesAngleJitter
#[test]
fn smoothing_steadies_the_roll() {
    let angles = [0.2, 0.2, -0.6, 0.2, 0.2];
    let mut track = Track {
        id: 1,
        low_confidence: false,
        boxes: angles
            .iter()
            .enumerate()
            .map(|(k, &roll)| TrackedBox {
                frame: k as u64,
                bbox: Rect::new(50.0, 100.0, 40.0, 40.0).unwrap(),
                score: 0.9,
                interpolated: false,
                roll: Some(roll),
            })
            .collect(),
    };
    smooth_track(&mut track, 2);
    assert!(track.boxes[2].roll.unwrap().abs() < 0.1);
}

/// testSmoothingDoesNotInflateMovingBoxes
#[test]
fn smoothing_does_not_inflate_moving_boxes() {
    let sequence = moving(30, 0.0, 12.0, 100.0, 0.9);
    let mut tracks = bidirectional(&sequence, &no_cuts());
    post_process_tracks(&mut tracks, &legacy_post(), 30, &no_cuts());
    assert_eq!(tracks.len(), 1);
    for (frame, detections) in sequence.iter().enumerate() {
        let processed = tracks[0].box_at(frame as u64).unwrap();
        if processed.interpolated {
            continue;
        }
        let detected = detections[0].bbox;
        assert!(processed.bbox.contains(&detected, 0.001));
        assert!(processed.bbox.area() <= detected.area() * 1.26);
    }
}

/// testSmoothingDoesNotInflateInterpolatedBoxes, and P-6 a: an interpolated box between two
/// large ones is not grown past a quarter more, and not shrunk either.
#[test]
fn smoothing_neither_inflates_nor_shrinks_interpolated_boxes() {
    let at = |frame, size: f32, offset: f32, interpolated| TrackedBox {
        frame,
        bbox: Rect::new(offset, offset, size, size).unwrap(),
        score: 0.9,
        interpolated,
        roll: None,
    };
    let mut track = Track {
        id: 1,
        low_confidence: false,
        boxes: vec![
            at(0, 200.0, 0.0, false),
            at(1, 50.0, 75.0, true),
            at(2, 200.0, 0.0, false),
        ],
    };
    let before = track.clone();
    smooth_track(&mut track, 2);
    assert!(track.boxes[1].bbox.area() <= 50.0 * 50.0 * 1.25 + 0.001);
    for (after, before) in track.boxes.iter().zip(&before.boxes) {
        assert!(after.bbox.contains(&before.bbox, 0.001));
    }
}

/// testSizeJumpStartsNewTrackInsteadOfAssociating
#[test]
fn a_jump_in_size_starts_a_new_track() {
    let sequence: Vec<Vec<Observation>> = (0..8)
        .map(|k| {
            vec![if k < 4 {
                det(100.0, 100.0, 0.9, 100.0)
            } else {
                det(65.0, 65.0, 0.9, 170.0)
            }]
        })
        .collect();
    let tracks = build_tracks(&sequence, &config(), &no_cuts());
    assert_eq!(tracks.len(), 2);
    for t in &tracks {
        assert!(
            t.boxes
                .iter()
                .all(|b| b.bbox.width() == t.boxes[0].bbox.width())
        );
    }
}

/// testGradualGrowthKeepsOneTrack
#[test]
fn gradual_growth_keeps_one_track() {
    let mut size = 40.0f32;
    let sequence: Vec<Vec<Observation>> = (0..10)
        .map(|_| {
            let half = size * 0.5;
            let d = det(150.0 - half, 150.0 - half, 0.9, size);
            size *= 1.1;
            vec![d]
        })
        .collect();
    assert_eq!(build_tracks(&sequence, &config(), &no_cuts()).len(), 1);
}

/// P-6 a (tests/fixtures/defects/P-6): interpolating the recorded boxes up to 20 frames and
/// smoothing with radius 2 leaves every box, detected or interpolated, covering what it covered
/// before. 1.12.1 shrank the interpolated (80, 80, 80, 80) of frames 10 and 11.
#[test]
fn smoothing_never_shrinks_a_box() {
    let defect = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/defects/P-6");
    let expected = read(defect.join("expected.json"));
    assert_eq!(expected["cases"][0]["part"], "a");
    let observed = read(defect.join("observed_1121.json"));
    let mut track = Track {
        id: 1,
        low_confidence: false,
        boxes: observed["a_smoothing_shrinks_interpolated_boxes"]["input_boxes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| TrackedBox {
                frame: u(&b["frame"]),
                bbox: rect(&b["box"]),
                score: f(&b["score"]),
                interpolated: false,
                roll: None,
            })
            .collect(),
    };
    assert!(interpolate_gaps(&mut track, 20, &no_cuts()).is_empty());
    let before = track.clone();
    smooth_track(&mut track, 2);
    assert_eq!(track.boxes.len(), before.boxes.len());
    for (after, before) in track.boxes.iter().zip(&before.boxes) {
        assert!(
            after.bbox.contains(&before.bbox, 0.0),
            "frame {}: {:?} does not hold {:?}",
            before.frame,
            after.bbox,
            before.bbox
        );
    }
    let interpolated: Vec<u64> = before
        .boxes
        .iter()
        .filter(|b| b.interpolated)
        .map(|b| b.frame)
        .collect();
    assert_eq!(interpolated, [10, 11, 12]);
}

/// Two tracks and two detections where taking the best pair first leaves the second track
/// without a box and starts a third: the optimal assignment continues both (plan 4.4).
#[test]
fn every_track_that_can_continue_does() {
    let frames = vec![
        vec![det(0.0, 0.0, 0.9, 40.0), det(20.0, 0.0, 0.9, 40.0)],
        // X overlaps A by 0.67 and B by 0.54; Y overlaps A by 0.6 and B too little.
        vec![det(8.0, 0.0, 0.9, 40.0), det(-10.0, 0.0, 0.9, 40.0)],
    ];
    let tracks = build_tracks(&frames, &config(), &no_cuts());
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].box_at(1).unwrap().bbox.x(), -10.0);
    assert_eq!(tracks[1].box_at(1).unwrap().bbox.x(), 8.0);
}

/// P-2 (tests/fixtures/defects/P-2): faces and plates are tracked apart, numbered from one
/// sequence, so a face and a plate overlapping by IoU 0.6 stay two tracks of one kind each.
#[test]
fn faces_and_plates_never_share_a_track() {
    use cloakframe_core::tracking::tracker::ByteTracker;
    let face = det(100.0, 100.0, 0.9, 40.0);
    let plate = Observation {
        bbox: Rect::new(110.0, 100.0, 40.0, 40.0).unwrap(),
        ..face
    };
    assert!((face.bbox.iou(&plate.bbox) - 0.6).abs() < 0.001);
    let mut faces = ByteTracker::new(config(), 1);
    let mut plates = ByteTracker::new(config(), 1_000_000);
    let mut ids = std::collections::BTreeSet::new();
    for k in 0..10 {
        for (tracker, d) in [(&mut faces, face), (&mut plates, plate)] {
            for a in tracker.update(k, false, &[d]).assigned {
                ids.insert(a.track);
            }
        }
    }
    assert_eq!(ids.into_iter().collect::<Vec<_>>(), [1, 1_000_000]);
}
