//! Global observations, never authority or proof that a Page can be reclaimed.
//! All quantities are bytes. Retention quantities count obligations, so aliases
//! may count a physical byte more than once. UNKNOWN never means zero.
use core::sync::atomic::{AtomicU64, Ordering};
pub const UNKNOWN: u64 = u64::MAX;
pub const VERSION: u64 = 1;
const WORDS: usize = 13;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub epoch: u64,
    pub start_ns: u64,
    pub end_ns: u64,
    /// Allocator-managed capacity, excluding firmware, image and bootstrap memory.
    pub capacity: u64,
    pub available: u64,
    /// Allocated managed bytes, including kernel heap, page tables and stacks.
    pub resident: u64,
    pub cpu_retained: u64,
    pub io_retained: u64,
    pub retiring: u64,
    pub abandoned: u64,
    pub accessed: u64,
    pub dirty: u64,
}
/// One naturally aligned, little-endian RV64 Share page. Word 0 is an even/odd
/// publication sequence; words 1..=13 are VERSION then Snapshot fields in order.
/// Remaining words are reserved zero. A single writer owns publication. Reads
/// are bounded: None means not yet published, busy, or an unsupported version.
#[repr(C, align(4096))]
pub struct SnapshotPage {
    sequence: AtomicU64,
    words: [AtomicU64; WORDS],
    reserved: [u64; 512 - WORDS - 1],
}
impl Default for SnapshotPage {
    fn default() -> Self {
        Self::new()
    }
}
impl SnapshotPage {
    pub const fn new() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            words: [const { AtomicU64::new(0) }; WORDS],
            reserved: [0; 512 - WORDS - 1],
        }
    }
    /// Caller owns the sole producer role. Counter exhaustion stops publication.
    pub fn publish(&self, value: Snapshot) -> bool {
        let Some(next) = self.sequence.load(Ordering::SeqCst).checked_add(2) else {
            return false;
        };
        self.sequence.store(next - 1, Ordering::SeqCst);
        let words = [
            VERSION,
            value.epoch,
            value.start_ns,
            value.end_ns,
            value.capacity,
            value.available,
            value.resident,
            value.cpu_retained,
            value.io_retained,
            value.retiring,
            value.abandoned,
            value.accessed,
            value.dirty,
        ];
        for (target, value) in self.words.iter().zip(words) {
            target.store(value, Ordering::SeqCst);
        }
        self.sequence.store(next, Ordering::SeqCst);
        true
    }
    /// The Share must remain mapped throughout the call. Atomic fields avoid a
    /// data race with publication; sequence equality rejects mixed epochs.
    pub fn read(&self) -> Option<Snapshot> {
        let before = self.sequence.load(Ordering::SeqCst);
        if before == 0 || before & 1 != 0 {
            return None;
        }
        let w = self
            .words
            .each_ref()
            .map(|word| word.load(Ordering::SeqCst));
        if before != self.sequence.load(Ordering::SeqCst) || w[0] != VERSION {
            return None;
        }
        Some(Snapshot {
            epoch: w[1],
            start_ns: w[2],
            end_ns: w[3],
            capacity: w[4],
            available: w[5],
            resident: w[6],
            cpu_retained: w[7],
            io_retained: w[8],
            retiring: w[9],
            abandoned: w[10],
            accessed: w[11],
            dirty: w[12],
        })
    }
}
const _: () = assert!(core::mem::size_of::<SnapshotPage>() == 4096);
