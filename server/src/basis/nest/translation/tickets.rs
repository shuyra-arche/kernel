use alloc::collections::TryReserveError;
use alloc::vec::Vec;

/// Address occupancy stays retired until its translation batch is fenced.
/// Settled tickets form a prefix, independent of any later in-flight batch.
pub(crate) struct Tickets {
    entries: Vec<usize>,
    settled: usize,
}

impl Tickets {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            settled: 0,
        }
    }

    pub(crate) fn reserve(&mut self, count: usize) -> Result<(), TryReserveError> {
        self.entries.try_reserve(count)
    }

    pub(crate) fn retire(&mut self, ticket: usize) {
        if !self.entries.contains(&ticket) {
            self.entries.push(ticket);
        }
    }

    /// Called under the Nest lock on successful completion of the current fence.
    pub(crate) fn settle(&mut self) {
        self.settled = self.entries.len();
    }

    pub(crate) fn has_settled(&self) -> bool {
        self.settled != 0
    }

    pub(crate) fn claim(&mut self, ticket: usize) -> bool {
        let Some(index) = self.entries[..self.settled]
            .iter()
            .position(|id| *id == ticket)
        else {
            return false;
        };
        self.entries.remove(index);
        self.settled -= 1;
        true
    }

    pub(crate) fn drain(&mut self) -> impl Iterator<Item = usize> + '_ {
        self.entries.drain(..core::mem::take(&mut self.settled))
    }
}
