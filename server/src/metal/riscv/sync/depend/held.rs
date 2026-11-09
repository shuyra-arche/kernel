use ::core::cell::UnsafeCell;
use alloc::vec::Vec;

use super::error::DepInitError;
use super::level::Level;

const MAX_HELD: usize = 8;

pub(super) struct HeldSet {
    pub(super) slots: Vec<Held>,
}

impl HeldSet {
    pub(super) fn new() -> Result<HeldSet, DepInitError> {
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(MAX_HELD)
            .map_err(|_| DepInitError::OutOfMemory)?;
        Ok(HeldSet { slots })
    }

    pub(super) fn snapshot(&self) -> Snapshot {
        let mut result = Snapshot {
            slots: [None; 32],
            omitted: self.slots.len().saturating_sub(32),
        };
        for (dst, src) in result.slots.iter_mut().zip(&self.slots) {
            *dst = Some(*src);
        }
        result
    }

    pub(super) fn push(
        &mut self,
        addr: usize,
        level: Option<Level>,
        caller: usize,
    ) -> Result<(), ()> {
        if self.slots.len() >= self.slots.capacity() {
            return Err(());
        }
        self.slots.push(Held {
            addr,
            level,
            caller,
        });
        Ok(())
    }

    pub(super) fn remove(&mut self, addr: usize) -> Result<(), ()> {
        for i in 0..self.slots.len() {
            if self.slots[i].addr == addr {
                self.slots.remove(i);
                return Ok(());
            }
        }
        Err(())
    }
}

#[derive(Clone, Copy)]
pub(super) struct Held {
    pub(super) addr: usize,
    pub(super) level: Option<Level>,
    pub(super) caller: usize,
}

pub(super) struct HeldCell(pub(super) UnsafeCell<HeldSet>);

unsafe impl Send for HeldCell {}
unsafe impl Sync for HeldCell {}

pub(super) struct Snapshot {
    slots: [Option<Held>; 32],
    omitted: usize,
}
impl core::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for slot in self.slots.iter().flatten() {
            write!(
                f,
                "\n lock={:#x} level={:?} acquired={:#x}",
                slot.addr, slot.level, slot.caller
            )?;
        }
        write!(f, "\n omitted={}", self.omitted)
    }
}
