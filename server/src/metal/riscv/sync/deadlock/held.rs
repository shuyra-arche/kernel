//! Fixed, per-Hart Debug storage. Acquisition/release never allocate or wait.
use super::super::order::Class;
use core::{cell::UnsafeCell, mem::MaybeUninit};
pub(super) const CAPACITY: usize = 64;
const _: () = assert!(super::super::order::MAX_DEPTH + 2 <= CAPACITY);
#[derive(Clone, Copy, Debug)]
pub(crate) enum Kind {
    Ordinary,
    Pair { predecessor: usize },
    SourceBorrow,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Held {
    pub addr: usize,
    pub class: Class,
    pub caller: usize,
    pub kind: Kind,
}
pub(super) struct HeldSet {
    slots: [MaybeUninit<Held>; CAPACITY],
    len: usize,
}
impl HeldSet {
    pub const fn new() -> Self {
        Self {
            slots: [MaybeUninit::uninit(); CAPACITY],
            len: 0,
        }
    }
    #[inline(always)]
    pub fn entries(&self) -> &[Held] {
        // Only push extends len, after writing that slot. Removal copies live
        // Copy metadata and shrinks len; no reference reaches an unwritten slot.
        unsafe { core::slice::from_raw_parts(self.slots.as_ptr().cast::<Held>(), self.len) }
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn push(&mut self, entry: Held) -> Result<(), ()> {
        if self.len == CAPACITY {
            return Err(());
        }
        self.slots[self.len].write(entry);
        self.len += 1;
        Ok(())
    }
    pub fn remove(&mut self, addr: usize) -> Result<(), ()> {
        if self
            .entries()
            .iter()
            .any(|h| matches!(h.kind,Kind::Pair{predecessor} if predecessor==addr))
        {
            return Err(());
        }
        let index = self
            .entries()
            .iter()
            .position(|h| h.addr == addr)
            .ok_or(())?;
        self.slots.copy_within(index + 1..self.len, index);
        self.len -= 1;
        Ok(())
    }
    pub fn snapshot(&self) -> Snapshot {
        let mut result = Snapshot {
            slots: [None; 16],
            omitted: self.len.saturating_sub(16),
        };
        let start = self.len.saturating_sub(16);
        for (to, from) in result
            .slots
            .iter_mut()
            .zip(&self.entries()[start..self.len])
        {
            *to = Some(*from);
        }
        result
    }
}
pub(super) struct HeldCell(pub UnsafeCell<HeldSet>);
// Only the owning Hart, with IRQs masked, accesses its preallocated cell.
unsafe impl Send for HeldCell {}
unsafe impl Sync for HeldCell {}
pub(super) struct Snapshot {
    slots: [Option<Held>; 16],
    omitted: usize,
}
impl core::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for h in self.slots.iter().flatten() {
            write!(
                f,
                "\n lock={:#x} class={:?} acquired={:#x} kind={:?}",
                h.addr, h.class, h.caller, h.kind
            )?;
        }
        write!(f, "\n omitted={}", self.omitted)
    }
}
