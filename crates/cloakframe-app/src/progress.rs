//! A run's progress as one amount of work, and the time it may still take (plan 6).

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use cloakframe_core::item::{ItemId, ItemState};
use cloakframe_engine::events::Progress;
use cloakframe_engine::scan::MediaKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Overall {
    pub done: u64,
    pub total: u64,
}

impl Overall {
    pub fn fraction(self) -> Option<f32> {
        (self.total > 0).then(|| (self.done as f64 / self.total as f64) as f32)
    }
}

/// The work of every item in pixels, as the engine counts it. An item the engine has not
/// reached counts its estimate, or the average of the items of its kind that have one. An item
/// that stopped, or waits for review, has no more work in this run.
pub fn overall(
    items: &[(ItemId, PathBuf)],
    states: &BTreeMap<ItemId, ItemState>,
    progress: &BTreeMap<ItemId, Progress>,
    estimates: &BTreeMap<ItemId, u64>,
) -> Overall {
    let known = |item: &ItemId| {
        progress
            .get(item)
            .filter(|p| p.total > 0)
            .map(|p| p.total)
            .or_else(|| estimates.get(item).copied())
    };
    let kind = |path: &PathBuf| MediaKind::of(path);
    let average = |of: Option<MediaKind>| {
        let totals: Vec<u64> = items
            .iter()
            .filter(|(_, path)| of.is_none() || kind(path) == of)
            .filter_map(|(item, _)| known(item))
            .collect();
        (!totals.is_empty()).then(|| totals.iter().sum::<u64>() / totals.len() as u64)
    };
    let mut overall = Overall::default();
    for (item, path) in items {
        let state = states.get(item).copied().unwrap_or(ItemState::Queued);
        let done = progress.get(item).map_or(0, |p| p.done);
        if state.is_terminal() || state == ItemState::NeedsReview {
            overall.done += done;
            overall.total += done;
            continue;
        }
        let total = known(item)
            .or_else(|| average(kind(path)))
            .or_else(|| average(None))
            .unwrap_or(0);
        overall.done += done.min(total);
        overall.total += total.max(done);
    }
    overall
}

/// How fast the work went lately, from which the time left follows. Over a window so a slow
/// start or a change of kind of item is forgotten.
#[derive(Debug)]
pub struct Pace {
    started: Instant,
    samples: VecDeque<(Instant, u64)>,
}

/// The work goes on long enough before the time left is shown that the pace means something.
const SETTLE: Duration = Duration::from_secs(5);
const WINDOW: Duration = Duration::from_secs(30);
const MIN_FRACTION: f64 = 0.01;

impl Pace {
    pub fn new(started: Instant) -> Self {
        Self {
            started,
            samples: VecDeque::new(),
        }
    }

    pub fn record(&mut self, now: Instant, done: u64) {
        if self.samples.back().is_some_and(|&(_, last)| done < last) {
            // Less done than before: an estimate turned out too big. The old samples measure
            // nothing now.
            self.samples.clear();
        }
        self.samples.push_back((now, done));
        while self
            .samples
            .front()
            .is_some_and(|&(at, _)| now.duration_since(at) > WINDOW)
            && self.samples.len() > 2
        {
            self.samples.pop_front();
        }
    }

    /// `None` until the run has gone on a while and done a little.
    pub fn left(&self, now: Instant, overall: Overall) -> Option<Duration> {
        if now.duration_since(self.started) < SETTLE
            || overall.total == 0
            || (overall.done as f64) < overall.total as f64 * MIN_FRACTION
        {
            return None;
        }
        let (&(first, from), &(last, to)) = (self.samples.front()?, self.samples.back()?);
        let seconds = last.duration_since(first).as_secs_f64();
        if seconds <= 0.0 || to <= from {
            return None;
        }
        let rate = (to - from) as f64 / seconds;
        Some(Duration::from_secs_f64(
            overall.total.saturating_sub(overall.done) as f64 / rate,
        ))
    }
}

/// The time left as the window says it: under a minute, minutes rounded up, or hours and
/// minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Left {
    UnderAMinute,
    Minutes(u64),
    Hours { hours: u64, minutes: u64 },
}

impl Left {
    pub fn of(left: Duration) -> Self {
        let minutes = left.as_secs().div_ceil(60);
        match minutes {
            0 | 1 if left < Duration::from_secs(60) => Self::UnderAMinute,
            m if m < 60 => Self::Minutes(m),
            m => Self::Hours {
                hours: m / 60,
                minutes: m % 60,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<(ItemId, PathBuf)> {
        ["a.jpg", "b.jpg", "c.mp4", "d.jpg"]
            .iter()
            .enumerate()
            .map(|(i, name)| (ItemId(i as u64), PathBuf::from(name)))
            .collect()
    }

    fn p(done: u64, total: u64) -> Progress {
        Progress { done, total }
    }

    #[test]
    fn items_count_their_pixels_estimates_or_their_kinds_average() {
        let items = items();
        let mut states = BTreeMap::new();
        let mut progress = BTreeMap::new();
        let mut estimates = BTreeMap::new();
        assert_eq!(
            overall(&items, &states, &progress, &estimates),
            Overall::default()
        );
        progress.insert(ItemId(0), p(50, 200));
        estimates.insert(ItemId(2), 10_000);
        // b.jpg and d.jpg count a.jpg's 200, the only photo with a total.
        assert_eq!(
            overall(&items, &states, &progress, &estimates),
            Overall {
                done: 50,
                total: 200 + 200 + 10_000 + 200
            }
        );
        estimates.insert(ItemId(1), 400);
        estimates.insert(ItemId(0), 999);
        // The engine's own total wins over the estimate; d.jpg averages a.jpg and b.jpg.
        assert_eq!(
            overall(&items, &states, &progress, &estimates).total,
            200 + 400 + 10_000 + 300
        );
        // Stopped or held for review: no work left, whatever was done counts as all of it.
        states.insert(ItemId(0), ItemState::NeedsReview);
        states.insert(ItemId(2), ItemState::Failed);
        progress.insert(ItemId(2), p(1_000, 20_000));
        assert_eq!(
            overall(&items, &states, &progress, &estimates),
            Overall {
                done: 50 + 1_000,
                total: 50 + 400 + 1_000 + 300
            }
        );
    }

    #[test]
    fn the_time_left_waits_for_a_pace_and_follows_the_latest_one() {
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        let total = 1_000;
        let mut pace = Pace::new(start);
        pace.record(at(0), 0);
        pace.record(at(2), 100);
        // Too early, however much is done.
        assert_eq!(pace.left(at(2), Overall { done: 100, total }), None);
        pace.record(at(10), 100);
        pace.record(at(20), 200);
        assert_eq!(
            pace.left(at(20), Overall { done: 200, total }),
            Some(Duration::from_secs(80))
        );
        // Ten times faster for the last half minute: the window forgets the slow start.
        for s in (25..=60).step_by(5) {
            pace.record(at(s), 200 + (s - 20) * 10);
        }
        let left = pace.left(at(60), Overall { done: 600, total }).unwrap();
        assert!((left.as_secs_f64() - 40.0).abs() < 4.0, "{left:?}");
        // Under one percent done: nothing to go on.
        let mut slow = Pace::new(start);
        slow.record(at(0), 0);
        slow.record(at(10), 5);
        assert_eq!(slow.left(at(10), Overall { done: 5, total }), None);
    }

    #[test]
    fn less_done_than_before_starts_the_pace_again() {
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        let mut pace = Pace::new(start);
        pace.record(at(0), 0);
        pace.record(at(10), 500);
        pace.record(at(11), 400);
        assert_eq!(
            pace.left(
                at(11),
                Overall {
                    done: 400,
                    total: 800
                }
            ),
            None
        );
        pace.record(at(12), 500);
        assert_eq!(
            pace.left(
                at(12),
                Overall {
                    done: 500,
                    total: 800
                }
            ),
            Some(Duration::from_secs(3))
        );
    }

    #[test]
    fn the_time_left_reads_in_minutes_or_hours() {
        let s = Duration::from_secs;
        assert_eq!(Left::of(s(0)), Left::UnderAMinute);
        assert_eq!(Left::of(s(59)), Left::UnderAMinute);
        assert_eq!(Left::of(s(60)), Left::Minutes(1));
        assert_eq!(Left::of(s(61)), Left::Minutes(2));
        assert_eq!(Left::of(s(59 * 60)), Left::Minutes(59));
        assert_eq!(
            Left::of(s(59 * 60 + 1)),
            Left::Hours {
                hours: 1,
                minutes: 0
            }
        );
        assert_eq!(
            Left::of(s(150 * 60)),
            Left::Hours {
                hours: 2,
                minutes: 30
            }
        );
    }
}
