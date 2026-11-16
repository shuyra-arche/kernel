//! Shared Nest admission: the one fact every Span observes before accepting work.
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU8, Ordering};

const OPEN: u8 = 0;
const CLOSING: u8 = 1;
const CLOSED: u8 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Phase {
    Open,
    Closing,
    Closed,
}

/// A cheap shared observation, not a second lifecycle manager.
///
/// Final acceptance still happens under the relevant commit/lock; this atomic
/// only lets a Span reject new work without locating its Nest.
pub(crate) struct Admission {
    phase: AtomicU8,
}

impl Admission {
    pub(crate) fn new() -> Result<Arc<Self>, super::NestError> {
        Arc::try_new(Self {
            phase: AtomicU8::new(OPEN),
        })
        .map_err(|_| super::NestError::OutOfMemory)
    }

    pub(crate) fn phase(&self) -> Phase {
        match self.phase.load(Ordering::Acquire) {
            OPEN => Phase::Open,
            CLOSING => Phase::Closing,
            _ => Phase::Closed,
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.phase() == Phase::Open
    }

    /// Close admission once; a repeated close is not an error.
    pub(crate) fn close(&self) -> bool {
        self.phase
            .compare_exchange(OPEN, CLOSING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Finish the close only after the translation has settled.
    pub(crate) fn closed(&self) {
        let _ = self
            .phase
            .compare_exchange(CLOSING, CLOSED, Ordering::AcqRel, Ordering::Acquire);
    }
}
