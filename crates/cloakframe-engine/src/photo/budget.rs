use std::fmt;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::cancel::CancelToken;

/// Working memory one image is expected to need: decoded storage, the detection copy, the render
/// buffer and the encoder's state, plus the encoded bytes held while it is processed (1.12.1's
/// estimate, which held up for its pipelines).
pub fn estimated_bytes(pixels: u64, encoded: u64) -> u64 {
    pixels.saturating_mul(16).saturating_add(encoded)
}

/// Shared limit on the memory images in flight may use, so parallel items cannot together exhaust
/// RAM. An image that alone exceeds the limit is refused; others wait for room.
#[derive(Debug, Clone)]
pub struct MemoryBudget(Arc<Inner>);

#[derive(Debug)]
struct Inner {
    capacity: u64,
    used: Mutex<u64>,
    freed: Condvar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetError {
    TooLarge { needed: u64, capacity: u64 },
    Cancelled,
}

impl fmt::Display for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { needed, capacity } => write!(
                f,
                "the image needs about {} MiB, more than the {} MiB this computer allows for one image",
                needed >> 20,
                capacity >> 20
            ),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for BudgetError {}

#[derive(Debug)]
pub struct MemoryPermit {
    budget: MemoryBudget,
    bytes: u64,
}

impl Drop for MemoryPermit {
    fn drop(&mut self) {
        let inner = &self.budget.0;
        *inner.used.lock().unwrap_or_else(|e| e.into_inner()) -= self.bytes;
        inner.freed.notify_all();
    }
}

impl MemoryBudget {
    pub fn new(capacity: u64) -> Self {
        Self(Arc::new(Inner {
            capacity,
            used: Mutex::new(0),
            freed: Condvar::new(),
        }))
    }

    /// A quarter of physical memory, kept between 512 MiB and 16 GiB (1.12.1's rule).
    pub fn for_this_computer() -> Self {
        const MIN: u64 = 512 << 20;
        const MAX: u64 = 16 << 30;
        Self::new(physical_memory().map_or(MIN, |total| (total / 4).clamp(MIN, MAX)))
    }

    pub fn capacity(&self) -> u64 {
        self.0.capacity
    }

    pub fn acquire(&self, bytes: u64, cancel: &CancelToken) -> Result<MemoryPermit, BudgetError> {
        let inner = &self.0;
        if bytes > inner.capacity {
            return Err(BudgetError::TooLarge {
                needed: bytes,
                capacity: inner.capacity,
            });
        }
        let mut used = inner.used.lock().unwrap_or_else(|e| e.into_inner());
        while *used + bytes > inner.capacity {
            if cancel.is_cancelled() {
                return Err(BudgetError::Cancelled);
            }
            used = inner
                .freed
                .wait_timeout(used, Duration::from_millis(50))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        *used += bytes;
        Ok(MemoryPermit {
            budget: self.clone(),
            bytes,
        })
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn physical_memory() -> Option<u64> {
    let mut bytes: u64 = 0;
    let mut size = size_of::<u64>();
    // SAFETY: hw.memsize is a u64; `bytes` and `size` describe a buffer of exactly that size.
    let ok = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&raw mut bytes).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    (ok == 0 && bytes > 0).then_some(bytes)
}

#[cfg(all(unix, not(target_os = "macos")))]
#[allow(unsafe_code)]
fn physical_memory() -> Option<u64> {
    // SAFETY: sysconf only reads the two configuration values.
    let (pages, page_size) = unsafe {
        (
            libc::sysconf(libc::_SC_PHYS_PAGES),
            libc::sysconf(libc::_SC_PAGE_SIZE),
        )
    };
    let pages = u64::try_from(pages).ok().filter(|&p| p > 0)?;
    pages.checked_mul(u64::try_from(page_size).ok().filter(|&s| s > 0)?)
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn physical_memory() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: MEMORYSTATUSEX is plain data; dwLength must be set before the call fills it in.
    unsafe {
        let mut status: MEMORYSTATUSEX = std::mem::zeroed();
        status.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
        (GlobalMemoryStatusEx(&mut status) != 0 && status.ullTotalPhys > 0)
            .then_some(status.ullTotalPhys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_like_1121_and_saturates() {
        assert_eq!(estimated_bytes(4, 100), 164);
        assert_eq!(estimated_bytes(u64::MAX, 1), u64::MAX);
        assert_eq!(estimated_bytes(u64::MAX, u64::MAX), u64::MAX);
    }

    #[test]
    fn the_computer_budget_is_a_clamped_quarter_of_ram() {
        let total = physical_memory().expect("the test machine reports its memory");
        let budget = MemoryBudget::for_this_computer().capacity();
        assert_eq!(budget, (total / 4).clamp(512 << 20, 16 << 30));
    }

    #[test]
    fn refuses_what_can_never_fit_and_waits_for_room_otherwise() {
        let budget = MemoryBudget::new(100);
        let cancel = CancelToken::new();
        assert_eq!(
            budget.acquire(101, &cancel).unwrap_err(),
            BudgetError::TooLarge {
                needed: 101,
                capacity: 100
            }
        );
        let first = budget.acquire(70, &cancel).unwrap();
        std::thread::scope(|s| {
            let waiter = s.spawn(|| budget.acquire(60, &cancel).map(|_| ()));
            std::thread::sleep(Duration::from_millis(30));
            assert!(!waiter.is_finished());
            drop(first);
            waiter.join().unwrap().unwrap();
        });
    }

    #[test]
    fn a_waiting_acquire_stops_when_cancelled() {
        let budget = MemoryBudget::new(100);
        let _held = budget.acquire(100, &CancelToken::new()).unwrap();
        let cancel = CancelToken::new();
        cancel.cancel();
        assert_eq!(
            budget.acquire(1, &cancel).unwrap_err(),
            BudgetError::Cancelled
        );
    }
}
