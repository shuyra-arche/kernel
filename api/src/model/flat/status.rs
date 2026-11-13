//! Next-memory-ABI observations. These are snapshots, never live authorization
//! or proof of hardware retirement. Current ABI v10 still uses FlatInfo.

use super::{BlockPhase, BlockRef, FlatId, SlotVersion};
use crate::model::{cell::CellLevel, identity::TaskId};
use core::num::NonZeroU64;

/// Flat structural/lifecycle revision and high-water mark for Slot stamps.
/// Each changed Slot receives the next Flat revision; unchanged Slots keep their
/// stamps. Neither revision tracks ordinary memory load/store content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FlatVersion(NonZeroU64);

impl FlatVersion {
    pub const INITIAL: Self = Self(NonZeroU64::MIN);
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
    pub const fn get(self) -> u64 {
        self.0.get()
    }
    pub fn next(self) -> Option<Self> {
        Self::new(self.get().checked_add(1)?)
    }

    /// Proposes one new Flat revision and a common stamp for every changed Slot.
    /// The live Flat must check this against its current revision and install
    /// both atomically. Never derive a revived child stamp from its old parent
    /// or from a deleted child record. This helper performs no state mutation.
    pub fn next_slots(self) -> Option<(Self, SlotVersion)> {
        let next = self.next()?;
        Some((next, SlotVersion::new(next.get())?))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum FlatPhase {
    Open = 0,
    Closing = 1,
    Closed = 2,
    Failed = 3,
}

impl FlatPhase {
    pub const fn from_tag(tag: u64) -> Option<Self> {
        match tag {
            0 => Some(Self::Open),
            1 => Some(Self::Closing),
            2 => Some(Self::Closed),
            3 => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlatStatus {
    pub id: FlatId,
    pub version: FlatVersion,
    /// Current effective scope, in fixed Flat-origin coordinates.
    pub offset: usize,
    pub count: usize,
    pub phase: FlatPhase,
}

impl FlatStatus {
    pub fn valid(self) -> bool {
        crate::model::cell::pages(self.offset, self.count).is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum SlotPhase {
    Fresh = 0,
    Resident = 1,
    Missing = 2,
    Occupying = 3,
    Moving = 4,
    Vacating = 5,
    Failed = 6,
}

impl SlotPhase {
    pub const fn from_tag(tag: u64) -> Option<Self> {
        match tag {
            0 => Some(Self::Fresh),
            1 => Some(Self::Resident),
            2 => Some(Self::Missing),
            3 => Some(Self::Occupying),
            4 => Some(Self::Moving),
            5 => Some(Self::Vacating),
            6 => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotStatus {
    pub flat: FlatId,
    pub coordinate: usize,
    pub version: SlotVersion,
    pub level: CellLevel,
    pub phase: SlotPhase,
    /// Current unique holding, or zero. Historical publications may retain a
    /// different Page after a move; obligations below include those as well.
    pub page: u64,
    pub obligations: u64,
    pub blocked: bool,
}

impl SlotStatus {
    pub fn valid(self) -> bool {
        let Some(grain) = self.level.pages() else {
            return false;
        };
        self.level.fits_at(self.coordinate, self.coordinate, grain)
            && self.page <= isize::MAX as u64
            && match self.phase {
                SlotPhase::Fresh | SlotPhase::Occupying => self.page == 0 && self.obligations == 0,
                SlotPhase::Missing => self.page == 0,
                SlotPhase::Resident | SlotPhase::Vacating => self.page != 0,
                SlotPhase::Moving | SlotPhase::Failed => true,
            }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockStatus {
    pub block: BlockRef,
    pub phase: BlockPhase,
    pub offset: usize,
    pub count: usize,
    pub actor: TaskId,
    /// Includes CPU and External responsibilities. Counts are observations,
    /// not completion tokens and not permission to reuse the allocation.
    pub ordinary: u64,
    pub maintenance: u64,
    pub failed: bool,
}

impl BlockStatus {
    pub fn valid(self) -> bool {
        crate::model::cell::pages(self.offset, self.count).is_some()
            && self.actor.get() > 0
            && self.actor.get() <= isize::MAX as usize
            && (self.phase != BlockPhase::Held || self.ordinary == 0)
            && (self.phase != BlockPhase::Closed
                || (self.ordinary == 0 && self.maintenance == 0 && !self.failed))
    }
}
