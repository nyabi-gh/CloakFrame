use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use cloakframe_core::item::{ItemId, ItemState};

use crate::run::RunReport;

/// Events that must all arrive, in order. They travel on a bounded channel, so a consumer that
/// stops reading eventually pauses the run instead of growing a queue.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    ItemState { item: ItemId, state: ItemState },
    RunFinished(RunReport),
}

/// Work done on one item, in pixels: a photo's for each of its two steps, a video's frame pixels
/// for each source frame of each pass. Items of every kind count alike, so a run's progress can
/// add them up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
}

/// Latest progress per item. Each update replaces the previous one, so progress never queues up
/// behind state events or slows the work that reports it.
#[derive(Debug, Clone, Default)]
pub struct ProgressBoard(Arc<Mutex<BTreeMap<ItemId, Progress>>>);

impl ProgressBoard {
    pub fn snapshot(&self) -> BTreeMap<ItemId, Progress> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn reporter(&self, item: ItemId) -> ProgressReporter {
        ProgressReporter {
            board: self.clone(),
            item,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProgressReporter {
    board: ProgressBoard,
    item: ItemId,
}

impl ProgressReporter {
    pub fn set(&self, done: u64, total: u64) {
        self.board
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(self.item, Progress { done, total });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_latest_progress_per_item() {
        let board = ProgressBoard::default();
        let a = board.reporter(ItemId(1));
        let b = board.reporter(ItemId(2));
        for done in 0..=100 {
            a.set(done, 100);
        }
        b.set(3, 9);
        let snapshot = board.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(
            snapshot[&ItemId(1)],
            Progress {
                done: 100,
                total: 100
            }
        );
        assert_eq!(snapshot[&ItemId(2)], Progress { done: 3, total: 9 });
    }
}
