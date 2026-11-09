use kernel_api::abi::call::unit::NOTE_MAX;
use kernel_api::model::{exit::Reason, identity::TaskId};
const CAPACITY: usize = 8;
pub(super) struct Ring {
    entries: [Entry; CAPACITY],
    next: usize,
    len: usize,
    overwritten: usize,
}
impl Ring {
    pub(super) const fn new() -> Self {
        Self {
            entries: [Entry::EMPTY; CAPACITY],
            next: 0,
            len: 0,
            overwritten: 0,
        }
    }
    pub(super) fn push(&mut self, entry: Entry) {
        self.entries[self.next] = entry;
        self.next = (self.next + 1) % CAPACITY;
        if self.len == CAPACITY {
            self.overwritten = self.overwritten.saturating_add(1);
        } else {
            self.len += 1;
        }
    }
    pub(super) fn snapshot(&self) -> Snapshot {
        let mut result = Snapshot {
            entries: [Entry::EMPTY; CAPACITY],
            len: self.len,
            overwritten: self.overwritten,
        };
        let start = if self.len == CAPACITY { self.next } else { 0 };
        for i in 0..self.len {
            result.entries[i] = self.entries[(start + i) % CAPACITY];
        }
        result
    }
}
pub(crate) struct Snapshot {
    entries: [Entry; CAPACITY],
    len: usize,
    pub overwritten: usize,
}
impl Snapshot {
    pub(crate) fn entries(&self) -> &[Entry] {
        &self.entries[..self.len]
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Entry {
    pub task: TaskId,
    pub reason: Reason,
    bytes: [u8; NOTE_MAX],
    len: usize,
}
impl Entry {
    const EMPTY: Self = Self {
        task: TaskId::new(0),
        reason: 0,
        bytes: [0; NOTE_MAX],
        len: 0,
    };
    pub(super) fn new(task: TaskId, reason: Reason, note: &str) -> Self {
        let mut len = note.len().min(NOTE_MAX);
        while !note.is_char_boundary(len) {
            len -= 1;
        }
        let mut entry = Self {
            task,
            reason,
            bytes: [0; NOTE_MAX],
            len,
        };
        entry.bytes[..len].copy_from_slice(&note.as_bytes()[..len]);
        entry
    }
    pub fn note(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("<invalid>")
    }
}
