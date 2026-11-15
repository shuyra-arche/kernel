//! Fixed, per-Hart Debug storage. Acquisition/release never allocate or wait.
use core::{cell::UnsafeCell, mem::MaybeUninit};
pub(crate) const CAPACITY: usize = 64;
#[derive(Clone, Copy, Debug)]
pub(crate) enum Kind {
    Ordinary,
    Pair { predecessor: usize },
    SourceBorrow,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Held<C> {
    pub addr: usize,
    pub class: C,
    pub caller: usize,
    pub kind: Kind,
}
pub(crate) struct HeldSet<C: Copy> {
    slots: [MaybeUninit<Held<C>>; CAPACITY],
    len: usize,
}
impl<C: Copy> HeldSet<C> {
    pub const fn new() -> Self {
        Self {
            slots: [MaybeUninit::uninit(); CAPACITY],
            len: 0,
        }
    }
    #[inline(always)]
    pub fn entries(&self) -> &[Held<C>] {
        // Only push extends len, after writing that slot. Removal copies live
        // Copy metadata and shrinks len; no reference reaches an unwritten slot.
        unsafe { core::slice::from_raw_parts(self.slots.as_ptr().cast::<Held<C>>(), self.len) }
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn push(&mut self, entry: Held<C>) -> Result<(), ()> {
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
    pub fn snapshot(&self) -> Snapshot<C> {
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
pub(crate) struct HeldCell<C: Copy>(pub UnsafeCell<HeldSet<C>>);
// Only the owning Hart, with IRQs masked, accesses its preallocated cell.
unsafe impl<C: Copy + Send> Send for HeldCell<C> {}
unsafe impl<C: Copy + Send> Sync for HeldCell<C> {}
pub(crate) struct Snapshot<C> {
    slots: [Option<Held<C>>; 16],
    omitted: usize,
}
impl<C: core::fmt::Debug> core::fmt::Debug for Snapshot<C> {
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

pub(crate) fn strict<C: Copy + PartialEq>(
    held: impl IntoIterator<Item = (usize, C)>,
    addr: usize,
    class: C,
    before: impl Fn(C, C) -> bool,
) -> Result<(), &'static str> {
    for (at, parent) in held {
        if at == addr {
            return Err("recursive Reject acquisition");
        }
        if !before(parent, class) {
            return Err("class is not reachable from every held ancestor");
        }
    }
    Ok(())
}

pub(crate) fn peer<C: Copy + PartialEq>(
    held: impl IntoIterator<Item = (usize, C)>,
    addr: usize,
    predecessor: usize,
    class: C,
    before: impl Fn(C, C) -> bool,
) -> Result<(), &'static str> {
    let (mut same, mut found) = (0, false);
    for (at, parent) in held {
        if at == addr {
            return Err("pair aliases its real predecessor");
        }
        if parent == class {
            same += 1;
            found = at == predecessor;
        } else if !before(parent, class) {
            return Err("pair under an incomparable or later ancestor");
        }
    }
    if same != 1 || !found {
        return Err("pair requires exactly its one real predecessor");
    }
    Ok(())
}
