//! D12 through the whole photo pipeline (read, decode, color policy, detection, mask, metadata,
//! encode, publish): nothing is written to the system temp folder. d12_no_source_copy.rs covers
//! the publish module alone. One test per binary, because it changes the environment.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cloakframe_core::detection::{Detection, ModelId, Thresholds};
use cloakframe_core::geometry::Rect;
use cloakframe_core::item::{ItemId, ItemState, PublishBasis, ReviewMode};
use cloakframe_engine::detect::{DetectError, Detector, DetectorHandle, Found, Rgb8Image};
use cloakframe_engine::photo::metadata::MetadataPolicy;
use cloakframe_engine::photo::process::{PhotoProcessor, PhotoSettings};
use cloakframe_engine::run::{Job, JobItem, Start, start};
use common::fixtures;

struct OneFace;

impl Detector for OneFace {
    fn detect(&mut self, _: &Rgb8Image, _: &Thresholds) -> Result<Found, DetectError> {
        let bbox = Rect::new(4.0, 4.0, 16.0, 16.0).unwrap();
        Ok(Found {
            detections: vec![Detection::new(ModelId::Yolo5FaceN, 0.9, bbox, None, None).unwrap()],
            ..Found::default()
        })
    }
}

#[test]
#[allow(unsafe_code)]
fn photos_are_processed_without_touching_the_system_temp_folder() {
    let root = std::env::temp_dir().join(format!("cloakframe-d12-photo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let private_temp = root.join("tmp");
    std::fs::create_dir_all(&private_temp).unwrap();
    // SAFETY: this binary has a single test and sets the variables before starting any thread.
    unsafe {
        for name in ["TMPDIR", "TMP", "TEMP"] {
            std::env::set_var(name, &private_temp);
        }
    }
    assert_eq!(std::env::temp_dir(), private_temp);

    let defects = fixtures().join("defects");
    let inputs = [
        "D6/q90-420.jpg",
        "D6/lossless.png",
        "D6/lossless.tif",
        "D6/lossy-q75.webp",
        "D2/chart-wide-gamma18.jpg",
    ];
    let items: Vec<JobItem> = inputs
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let file = Path::new(name).file_name().unwrap();
            let input = root.join("in").join(file);
            std::fs::create_dir_all(input.parent().unwrap()).unwrap();
            std::fs::copy(defects.join(name), &input).unwrap();
            JobItem {
                id: ItemId(i as u64),
                input,
                destination: root.join("out").join(file),
                start: Start::Fresh {
                    needs_review: false,
                },
            }
        })
        .collect();
    let processor = PhotoProcessor::new(PhotoSettings {
        metadata: MetadataPolicy::Allowlist { gps: false },
        ..PhotoSettings::default()
    })
    .unwrap();
    let job = Job {
        items,
        review: ReviewMode::Off,
        workers: 2,
    };
    let detector = DetectorHandle::spawn(Box::new(OneFace)).unwrap();
    let report = start(job, Arc::new(processor), detector).unwrap().wait();

    for (name, item) in inputs.iter().zip(&report.items) {
        assert_eq!(
            item.state,
            ItemState::Published(PublishBasis::AutomaticUnreviewed),
            "{name}: {:?}",
            item.failure
        );
    }
    let left: Vec<PathBuf> = std::fs::read_dir(&private_temp)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert!(left.is_empty(), "{left:?}");
    std::fs::remove_dir_all(root).unwrap();
}
