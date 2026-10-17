use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Cancellation flag shared between the caller and the work it started. A child token is
/// cancelled when it or any ancestor is, so a run token stops every item token under it.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<Node>);

#[derive(Debug, Default)]
struct Node {
    cancelled: AtomicBool,
    parent: Option<CancelToken>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn child(&self) -> Self {
        Self(Arc::new(Node {
            cancelled: AtomicBool::new(false),
            parent: Some(self.clone()),
        }))
    }

    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
            || self
                .0
                .parent
                .as_ref()
                .is_some_and(CancelToken::is_cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_cancels_children_but_not_the_reverse() {
        let run = CancelToken::new();
        let a = run.child();
        let b = run.child();
        a.cancel();
        assert!(a.is_cancelled());
        assert!(!b.is_cancelled() && !run.is_cancelled());
        run.cancel();
        assert!(b.is_cancelled() && b.child().is_cancelled());
    }
}
