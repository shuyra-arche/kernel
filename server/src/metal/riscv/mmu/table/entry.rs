//! RV64 PTE storage and coherent decoded snapshots. Software mutation remains
//! serialized by the Nest owner; hardware may atomically set A/D concurrently.
use core::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};

use super::flags::PteFlags;

#[repr(transparent)]
#[derive(Default)]
pub struct PageTableEntry {
    bits: AtomicU64,
}

/// One full-width observation, not a reference to mutable hardware state.
#[derive(Clone, Copy)]
pub struct Snapshot {
    bits: u64,
}

impl PageTableEntry {
    #[inline(always)]
    pub fn read(&self) -> Snapshot {
        Snapshot {
            bits: self.bits.load(Ordering::Acquire),
        }
    }

    /// Publish a prepared mapping/branch. Replacing a live leaf still requires
    /// the existing Nest invalidation and retirement protocol.
    #[inline(always)]
    pub fn set(&self, ppn: u64, flags: PteFlags) {
        self.bits.store(
            (ppn << Snapshot::PPN_SHIFT) | flags.bits(),
            Ordering::Release,
        );
    }

    /// Change software flags without losing hardware A/D updates. Clearing A/D
    /// must use a separate checked epoch/fence protocol; this is not that arm.
    #[inline(always)]
    pub fn set_flags(&self, flags: PteFlags) {
        let activity = (PteFlags::A | PteFlags::D).bits();
        self.bits
            .try_update(Ordering::AcqRel, Ordering::Acquire, |bits| {
                Some((bits & !Snapshot::FLAGS_MASK) | flags.bits() | (bits & activity))
            })
            .expect("flag update always supplies a value");
    }

    #[inline(always)]
    pub fn clear(&self) {
        self.bits.store(0, Ordering::Release);
    }
}

impl Snapshot {
    const FLAGS_MASK: u64 = 0x3FF;
    const PPN_SHIFT: usize = 10;

    #[inline(always)]
    pub fn is_valid(self) -> bool {
        self.flags().contains(PteFlags::V)
    }

    #[inline(always)]
    pub fn is_leaf(self) -> bool {
        self.flags()
            .intersects(PteFlags::R | PteFlags::W | PteFlags::X)
    }

    #[inline(always)]
    pub fn is_branch(self) -> bool {
        self.is_valid() && !self.is_leaf()
    }

    #[inline(always)]
    pub fn ppn(self) -> u64 {
        self.bits >> Self::PPN_SHIFT
    }

    #[inline(always)]
    pub fn paddr(self) -> u64 {
        self.ppn() << 12
    }

    #[inline(always)]
    pub fn flags(self) -> PteFlags {
        PteFlags::from_bits_truncate(self.bits & Self::FLAGS_MASK)
    }
}

impl fmt::Debug for PageTableEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.read();
        if !value.is_valid() {
            write!(f, "PTE(invalid)")
        } else if value.is_branch() {
            write!(f, "PTE(branch → {:#x})", value.paddr())
        } else {
            write!(
                f,
                "PTE({:#x}, ppn={:#x}, flags={:?})",
                value.bits,
                value.ppn(),
                value.flags()
            )
        }
    }
}
