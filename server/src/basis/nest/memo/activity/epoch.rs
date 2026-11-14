//! Memo-owned cumulative evidence. A fence failure retains the captured facts;
//! another reader never clears or acknowledges this owner's history.
use core::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub epoch: u64,
    pub accessed: u64,
    pub dirty: u64,
    pub start: u64,
    pub end: u64,
    pub synchronized: u64,
}
pub(super) enum Phase<R> {
    Cold,
    Armed {
        clear: u64,
        synchronized: u64,
    },
    Pending {
        round: Option<R>,
        bits: (bool, bool),
        start: Option<u64>,
        clear: u64,
    },
    Exhausted,
}
pub(crate) struct History<R> {
    pub(super) phase: Phase<R>,
    pub(super) snapshot: Snapshot,
}
impl<R> History<R> {
    pub(crate) const fn new() -> Self {
        Self {
            phase: Phase::Cold,
            snapshot: Snapshot {
                epoch: 0,
                accessed: 0,
                dirty: 0,
                start: 0,
                end: 0,
                synchronized: 0,
            },
        }
    }
    pub(super) fn capture(&mut self, bits: (bool, bool), clear: u64) {
        let start = match self.phase {
            Phase::Cold => None,
            Phase::Armed { clear, .. } => Some(clear),
            _ => panic!("only an unarmed or completed epoch can clear a PTE"),
        };
        self.phase = Phase::Pending {
            round: None,
            bits,
            start,
            clear,
        };
    }
    pub(super) fn complete(&mut self, synchronized: u64) {
        let Phase::Pending {
            bits, start, clear, ..
        } = self.phase
        else {
            panic!("exact pending epoch")
        };
        let Some(epoch) = NEXT
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            .ok()
            .and_then(|old| old.checked_add(1))
            .filter(|epoch| *epoch > self.snapshot.epoch)
        else {
            self.phase = Phase::Exhausted;
            return;
        };
        // The initial clear discards preset/pre-registration A/D, never exposes it.
        self.snapshot.epoch = epoch;
        self.snapshot.start = start.unwrap_or(clear);
        self.snapshot.end = clear;
        self.snapshot.synchronized = synchronized;
        if start.is_some() {
            if bits.0 {
                self.snapshot.accessed = epoch;
            }
            if bits.1 {
                self.snapshot.dirty = epoch;
            }
        }
        self.phase = Phase::Armed {
            clear,
            synchronized,
        };
    }
    pub(crate) fn snapshot(&self) -> Snapshot {
        self.snapshot
    }
}
