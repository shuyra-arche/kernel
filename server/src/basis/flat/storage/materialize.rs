//! Fresh-slot allocation responsibility. Metadata allocation, Page allocation
//! and destruction happen outside the IRQ-off Flat lock. Vec relocation and
//! sorted insertion still take O(number of slots) under that lock; this is not
//! a bounded-latency storage representation.
use super::{
    FlatSource, FlatState,
    slot::{self, Slot},
};
use crate::{
    basis::{cell::page::Page, flat::FlatError},
    metal::sync::SpinLock,
};
use alloc::{sync::Arc, vec::Vec};
use kernel_api::{
    address::PageIndex,
    model::{
        cell::{CellLevel, PageOwner},
        flat::FlatPhase,
    },
};

pub(in super::super) enum Step<'a> {
    Ready(usize),
    Allocate(Reservation<'a>),
}

/// Borrows the source lock, so the source outlives cancellation/publication.
/// No unpublished Page is stored here: the allocating caller owns and drops it
/// outside the lock on every failure. Cancellation restores only this Fresh
/// position, never a Missing tombstone or another transaction's slot.
pub(in super::super) struct Reservation<'a> {
    state: &'a SpinLock<FlatState>,
    index: PageIndex,
    level: CellLevel,
    initial: CellLevel,
    id: usize,
}

impl<'a> Reservation<'a> {
    pub fn prepare(
        state: &'a SpinLock<FlatState>,
        index: PageIndex,
        end: usize,
        initial: CellLevel,
    ) -> Result<Step<'a>, FlatError> {
        let mut spare = Vec::new();
        loop {
            let mut guard = state.lock();
            let (head, level) = guard.geometry.head(index.get(), initial);
            let pages = level.pages().unwrap();
            if head != index.get() || pages > end.saturating_sub(head) {
                return Err(FlatError::NotAligned);
            }
            if guard.phase != FlatPhase::Open {
                return Err(FlatError::Busy);
            }
            let explicit = guard.explicit;
            let FlatSource::Managed(slots) = &mut guard.source else {
                return Ok(Step::Ready(end - index.get()));
            };
            let (at, present) = slot::spot(slots, index);
            if present {
                return match slots[at].1 {
                    Slot::Resident(_) => Ok(Step::Ready(pages)),
                    Slot::Missing => Err(FlatError::Missing),
                    _ => Err(FlatError::Busy),
                };
            }
            if explicit {
                return Err(FlatError::Missing);
            }
            if slots.len() == slots.capacity() {
                let needed = slots.len().checked_add(1).ok_or(FlatError::OutOfMemory)?;
                if spare.capacity() < needed {
                    // Peers can grow the table while allocation runs. Recheck
                    // the position and capacity before using this proposal.
                    let capacity = slots
                        .capacity()
                        .checked_mul(2)
                        .unwrap_or(needed)
                        .max(needed);
                    drop(guard);
                    spare
                        .try_reserve(capacity)
                        .map_err(|_| FlatError::OutOfMemory)?;
                    continue;
                }
                spare.append(slots);
                core::mem::swap(slots, &mut spare);
                // spare now owns the old empty allocation. It is dropped only
                // after guard, including when identity allocation fails.
            }
            static NEXT: crate::basis::name::Serial =
                crate::basis::name::Serial::new(1, isize::MAX as usize);
            let id = NEXT.allocate().ok_or(FlatError::OutOfMemory)?;
            slots.insert(at, (index, Slot::Allocating { reservation: id }));
            return Ok(Step::Allocate(Self {
                state,
                index,
                level,
                initial,
                id,
            }));
        }
    }

    pub fn level(&self) -> CellLevel {
        self.level
    }

    pub fn publish(self, page: Page, owner: PageOwner) -> Result<(), FlatError> {
        // Consume the allocation itself, never an Arc borrowed from another
        // slot/mapping. Arc allocation remains outside the Flat state lock.
        let page = Arc::try_new(page).map_err(|_| FlatError::OutOfMemory)?;
        // Keep page owned by this frame until after releasing the guard, even
        // on rejection. A stale publisher cannot erase its replacement.
        let result = {
            let mut guard = self.state.lock();
            if guard.phase != FlatPhase::Open
                || guard.explicit
                || guard.geometry.head(self.index.get(), self.initial)
                    != (self.index.get(), self.level)
                || page.level() != self.level
                || page.owner() != owner
            {
                Err(FlatError::Busy)
            } else if let FlatSource::Managed(slots) = &mut guard.source {
                let (at, present) = slot::spot(slots, self.index);
                if present
                    && matches!(slots[at].1, Slot::Allocating { reservation } if reservation == self.id)
                {
                    slots[at].1 = Slot::Resident(page.clone());
                    Ok(())
                } else {
                    Err(FlatError::Busy)
                }
            } else {
                Err(FlatError::Busy)
            }
        };
        drop(page);
        result
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        let mut guard = self.state.lock();
        if let FlatSource::Managed(slots) = &mut guard.source {
            let (at, present) = slot::spot(slots, self.index);
            if present
                && matches!(slots[at].1, Slot::Allocating { reservation } if reservation == self.id)
            {
                slots.remove(at);
            }
        }
    }
}
