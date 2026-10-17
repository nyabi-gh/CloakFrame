use std::any::Any;
use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Condvar, Mutex};

use crate::cancel::CancelToken;

struct State<T, E> {
    next: usize,
    consumed: usize,
    active: usize,
    ready: BTreeMap<usize, T>,
    stopped: bool,
    error: Option<E>,
    panic: Option<Box<dyn Any + Send>>,
}

/// Stops the workers however the consumer exits, including by panicking, so the scope can join.
struct StopOnDrop<'a, T, E> {
    state: &'a Mutex<State<T, E>>,
    changed: &'a Condvar,
}

impl<T, E> Drop for StopOnDrop<'_, T, E> {
    fn drop(&mut self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).stopped = true;
        self.changed.notify_all();
    }
}

/// Runs `produce` for `0..count` on up to `threads` workers and hands each result to `consume`
/// in index order on the calling thread. At most `max_in_flight` results are produced ahead of
/// the consumer. Cancellation stops admitting new indices; results of producers that were already
/// running are still consumed (in index order, possibly with gaps) so they cannot vanish from a
/// report. The first error from either side stops the run and is returned.
pub fn process_ordered<T, E, P, C>(
    count: usize,
    threads: usize,
    max_in_flight: usize,
    cancel: &CancelToken,
    produce: P,
    mut consume: C,
) -> Result<(), E>
where
    T: Send,
    E: Send,
    P: Fn(usize) -> Result<T, E> + Sync,
    C: FnMut(usize, T) -> Result<(), E>,
{
    if count == 0 {
        return Ok(());
    }
    let threads = threads.clamp(1, count);
    let max_in_flight = max_in_flight.max(1);
    let state = Mutex::new(State {
        next: 0,
        consumed: 0,
        active: 0,
        ready: BTreeMap::new(),
        stopped: false,
        error: None,
        panic: None,
    });
    let changed = Condvar::new();
    let lock = || state.lock().unwrap_or_else(|e| e.into_inner());

    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let index = {
                        let mut s = changed
                            .wait_while(lock(), |s| {
                                !s.stopped && s.next < count && s.next >= s.consumed + max_in_flight
                            })
                            .unwrap_or_else(|e| e.into_inner());
                        if s.stopped || s.next >= count {
                            return;
                        }
                        if cancel.is_cancelled() {
                            s.stopped = true;
                            changed.notify_all();
                            return;
                        }
                        s.next += 1;
                        s.active += 1;
                        s.next - 1
                    };
                    let result = panic::catch_unwind(AssertUnwindSafe(|| produce(index)));
                    let mut s = lock();
                    s.active -= 1;
                    match result {
                        Ok(Ok(value)) => {
                            s.ready.insert(index, value);
                        }
                        Ok(Err(e)) => {
                            s.error.get_or_insert(e);
                            s.stopped = true;
                        }
                        Err(payload) => {
                            s.panic.get_or_insert(payload);
                            s.stopped = true;
                        }
                    }
                    changed.notify_all();
                }
            });
        }

        let _stop = StopOnDrop {
            state: &state,
            changed: &changed,
        };
        let mut s = lock();
        loop {
            s = changed
                .wait_while(s, |s| {
                    s.consumed < count
                        && !s.ready.contains_key(&s.consumed)
                        && !(s.stopped && s.active == 0)
                })
                .unwrap_or_else(|e| e.into_inner());
            if let Some(payload) = s.panic.take() {
                drop(s);
                panic::resume_unwind(payload);
            }
            if let Some(e) = s.error.take() {
                return Err(e);
            }
            if s.consumed == count {
                return Ok(());
            }
            let index = s.consumed;
            if let Some(value) = s.ready.remove(&index) {
                drop(s);
                let consumed = consume(index, value);
                s = lock();
                consumed?;
                s.consumed += 1;
                changed.notify_all();
                continue;
            }
            let leftovers = std::mem::take(&mut s.ready);
            drop(s);
            for (index, value) in leftovers {
                consume(index, value)?;
            }
            return Ok(());
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[derive(Debug, PartialEq)]
    struct Failed(&'static str);

    #[test]
    fn consumes_in_order() {
        let mut consumed = Vec::new();
        process_ordered(
            200,
            8,
            8,
            &CancelToken::new(),
            |i| {
                std::thread::sleep(Duration::from_micros(((i * 37) % 11) as u64 * 20));
                Ok::<_, Failed>(i * 2)
            },
            |i, value| {
                assert_eq!(value, i * 2);
                consumed.push(i);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(consumed, (0..200).collect::<Vec<_>>());
    }

    #[test]
    fn respects_the_in_flight_bound() {
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let mut consumed = 0;
        process_ordered(
            100,
            8,
            3,
            &CancelToken::new(),
            |i| {
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_micros(100));
                active.fetch_sub(1, Ordering::SeqCst);
                Ok::<_, Failed>(i)
            },
            |_, _| {
                consumed += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(consumed, 100);
        assert!(peak.load(Ordering::SeqCst) <= 3);
    }

    #[test]
    fn stops_after_cancellation() {
        let cancel = CancelToken::new();
        let mut consumed = 0;
        process_ordered(
            1000,
            4,
            4,
            &cancel,
            |i| {
                std::thread::sleep(Duration::from_micros(50));
                Ok::<_, Failed>(i)
            },
            |_, _| {
                consumed += 1;
                if consumed == 20 {
                    cancel.cancel();
                }
                Ok(())
            },
        )
        .unwrap();
        assert!((20..1000).contains(&consumed));
    }

    #[test]
    fn cancellation_keeps_results_from_running_producers() {
        let cancel = CancelToken::new();
        let started = Barrier::new(2);
        let mut consumed = Vec::new();
        process_ordered(
            10,
            2,
            2,
            &cancel,
            |i| {
                started.wait();
                if i == 0 {
                    cancel.cancel();
                } else {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Ok::<_, Failed>(i)
            },
            |i, value| {
                assert_eq!(i, value);
                consumed.push(i);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(consumed, [0, 1]);
    }

    #[test]
    fn single_thread_and_empty() {
        let mut consumed = 0;
        process_ordered(0, 4, 4, &CancelToken::new(), Ok::<_, Failed>, |_, _| {
            consumed += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(consumed, 0);
        process_ordered(5, 0, 0, &CancelToken::new(), Ok::<_, Failed>, |i, value| {
            assert_eq!(i, value);
            consumed += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(consumed, 5);
    }

    #[test]
    fn propagates_producer_errors() {
        let result = process_ordered(
            200,
            8,
            8,
            &CancelToken::new(),
            |i| {
                if i == 7 {
                    return Err(Failed("produce"));
                }
                std::thread::sleep(Duration::from_micros(50));
                Ok(i)
            },
            |_, _| Ok(()),
        );
        assert_eq!(result, Err(Failed("produce")));
    }

    #[test]
    #[should_panic(expected = "producer bug")]
    fn propagates_producer_panics() {
        let _ = process_ordered(
            50,
            4,
            2,
            &CancelToken::new(),
            |i| {
                assert_ne!(i, 9, "producer bug");
                Ok::<_, Failed>(i)
            },
            |_, _| Ok(()),
        );
    }

    #[test]
    #[should_panic(expected = "consumer bug")]
    fn consumer_panics_do_not_leave_workers_waiting() {
        let _ = process_ordered(50, 4, 2, &CancelToken::new(), Ok::<_, Failed>, |i, _| {
            assert_ne!(i, 3, "consumer bug");
            Ok(())
        });
    }

    #[test]
    fn propagates_consumer_errors() {
        let mut consumed = Vec::new();
        let result = process_ordered(200, 8, 8, &CancelToken::new(), Ok, |i, _| {
            if i == 5 {
                return Err(Failed("consume"));
            }
            consumed.push(i);
            Ok(())
        });
        assert_eq!(result, Err(Failed("consume")));
        assert_eq!(consumed, [0, 1, 2, 3, 4]);
    }
}
