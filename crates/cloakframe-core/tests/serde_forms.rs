//! Stored forms of the core types. Sessions written by one version are read by the next, so
//! these shapes change only together with a session format version.

use cloakframe_core::coverage::{CoverageEvidence, Problem, Resolution};
use cloakframe_core::detection::{Detection, ModelId};
use cloakframe_core::geometry::{Point, Rect};
use cloakframe_core::review::{Digest, ReviewKey, ReviewRevision};
use serde_json::json;

#[test]
fn detections_round_trip_and_invalid_ones_are_rejected() {
    let face = Detection::new(
        ModelId::Yolo5FaceN,
        0.75,
        Rect::new(1.0, 2.0, 3.0, 4.0).unwrap(),
        Some([Point { x: 1.5, y: 2.5 }; 5]),
        Some(12),
    )
    .unwrap();
    let stored = serde_json::to_value(&face).unwrap();
    assert_eq!(stored["model"], "yolo5_face_n");
    assert_eq!(stored["frame"], 12);
    assert_eq!(serde_json::from_value::<Detection>(stored).unwrap(), face);

    let plate = json!({"model": "plate_yolov9_t", "score": 0.5, "bbox": {"x": 0, "y": 0, "width": 4, "height": 2}});
    assert!(serde_json::from_value::<Detection>(plate.clone()).is_ok());
    for (field, value) in [
        ("score", json!(1.5)),
        ("bbox", json!({"x": 0, "y": 0, "width": -1, "height": 2})),
        ("landmarks", json!(vec![json!({"x": 0, "y": 0}); 5])),
    ] {
        let mut bad = plate.clone();
        bad[field] = value;
        assert!(serde_json::from_value::<Detection>(bad).is_err(), "{field}");
    }
}

#[test]
fn coverage_evidence_uses_the_fixture_kinds() {
    let gap = CoverageEvidence::unresolved(Problem::UnresolvedGap {
        track: 1,
        first: 30,
        last: 54,
    });
    let stored = serde_json::to_value(&gap).unwrap();
    assert_eq!(
        stored,
        json!({"kind": "unresolved_gap", "track": 1, "first": 30, "last": 54, "resolution": "unresolved"})
    );
    let mut capped =
        serde_json::to_value(CoverageEvidence::unresolved(Problem::CappedCandidates {
            model: ModelId::Yolo5FaceN,
            frame: None,
            kept: 100,
            found: 259,
        }))
        .unwrap();
    assert_eq!(capped["kind"], "capped_candidates");
    capped["resolution"] = json!("dismissed");
    let back: CoverageEvidence = serde_json::from_value(capped).unwrap();
    assert_eq!(back.resolution, Resolution::Dismissed);
    assert_eq!(
        serde_json::from_value::<CoverageEvidence>(stored).unwrap(),
        gap
    );
}

#[test]
fn digests_are_hex_and_revisions_round_trip() {
    let key = ReviewKey {
        input: Digest([0xab; 32]),
        models: Digest([0; 32]),
        settings: Digest([1; 32]),
        timeline: Digest([0xff; 32]),
    };
    let revision = ReviewRevision::first(key).next();
    let stored = serde_json::to_value(revision).unwrap();
    assert_eq!(stored["key"]["input"], "ab".repeat(32));
    assert_eq!(stored["number"], 1);
    assert_eq!(
        serde_json::from_value::<ReviewRevision>(stored).unwrap(),
        revision
    );
    assert!(serde_json::from_value::<Digest>(json!("ab")).is_err());
    assert!(serde_json::from_value::<Digest>(json!("zz".repeat(32))).is_err());
}
