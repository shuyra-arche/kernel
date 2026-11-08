use core::sync::atomic::{AtomicUsize, Ordering};

/// A non-reusing identity domain. Zero is the terminal, exhausted state.
pub(crate) struct Serial {
    next: AtomicUsize,
    last: usize,
}

impl Serial {
    pub(crate) const fn new(first: usize, last: usize) -> Self {
        assert!(first != 0 && first <= last);
        Self {
            next: AtomicUsize::new(first),
            last,
        }
    }

    pub(crate) fn allocate(&self) -> Option<usize> {
        self.next
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                if next == 0 {
                    None
                } else if next == self.last {
                    Some(0)
                } else {
                    Some(next + 1)
                }
            })
            .ok()
    }
}
