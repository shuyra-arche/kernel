//! Next-memory-ABI space, reservation and Memo observations. Descriptive
//! snapshots do not replace a live Whole/Reservation Pend or retirement evidence.

use super::{MemoGeneration, NestId, NestInfo};
use crate::address::{BASE_PAGE_SIZE, VirtAddr};
use crate::model::{cell::CellLevel, flat::FlatId, nest::ReservationId};
use core::num::NonZeroU64;

/// Address-space topology/lifecycle epoch, not a Memo publication generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NestVersion(NonZeroU64);
impl NestVersion {
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
}

/// Aggregate Reservation revision used by Release, distinct from one Memo's
/// generation. Changes anywhere in the reservation invalidate an old revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReservationVersion(NonZeroU64);
impl ReservationVersion {
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NestStatus {
    pub info: NestInfo,
    pub version: NestVersion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum ReservationPhase {
    Open = 0,
    Releasing = 1,
    Released = 2,
    Failed = 3,
}
impl ReservationPhase {
    pub const fn from_tag(tag: u64) -> Option<Self> {
        match tag {
            0 => Some(Self::Open),
            1 => Some(Self::Releasing),
            2 => Some(Self::Released),
            3 => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReservationStatus {
    pub nest: NestId,
    pub reservation: ReservationId,
    pub version: ReservationVersion,
    pub offset: usize,
    pub count: usize,
    /// Original reservation base, not the narrowed window's first address.
    pub address: VirtAddr,
    pub level: CellLevel,
    /// Creation access ceiling, intersected with this Pend's write ceiling.
    /// Per-position current publication access is reported by MemoStatus.
    pub access: u64,
    pub phase: ReservationPhase,
}
impl ReservationStatus {
    pub fn valid(self) -> bool {
        let Some(grain) = self.level.pages() else {
            return false;
        };
        self.reservation.get() > 0
            && self.reservation.get() <= isize::MAX as usize
            && crate::model::cell::pages(self.offset, self.count).is_some()
            && self.offset.is_multiple_of(grain)
            && self.count.is_multiple_of(grain)
            && self.address.get().is_multiple_of(BASE_PAGE_SIZE)
            && self
                .offset
                .checked_add(self.count)
                .and_then(|end| end.checked_mul(BASE_PAGE_SIZE))
                .and_then(|bytes| self.address.get().checked_add(bytes))
                .is_some()
            && valid_access(self.access)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum MemoPhase {
    Empty = 0,
    Associated = 1,
    Publishing = 2,
    Published = 3,
    Retiring = 4,
    Failed = 5,
}
impl MemoPhase {
    pub const fn from_tag(tag: u64) -> Option<Self> {
        match tag {
            0 => Some(Self::Empty),
            1 => Some(Self::Associated),
            2 => Some(Self::Publishing),
            3 => Some(Self::Published),
            4 => Some(Self::Retiring),
            5 => Some(Self::Failed),
            _ => None,
        }
    }
}

/// Describes the resolved original logical Flat; Share/Memo authority lineage
/// remains in Kernel. Equal source coordinates never establish equal authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoStatus {
    pub nest: NestId,
    pub reservation: ReservationId,
    pub coordinate: usize,
    pub generation: MemoGeneration,
    pub phase: MemoPhase,
    pub source: Option<(FlatId, usize)>,
    pub page: u64,
    /// Access of the physical publication; zero when there is no publication.
    pub access: u64,
    pub level: CellLevel,
}
impl MemoStatus {
    pub fn valid(self) -> bool {
        let Some(grain) = self.level.pages() else {
            return false;
        };
        if self.reservation.get() == 0
            || self.reservation.get() > isize::MAX as usize
            || !self.level.fits_at(self.coordinate, self.coordinate, grain)
            || self.page > isize::MAX as u64
            || self
                .source
                .is_some_and(|(_, page)| !self.level.fits_at(page, page, grain))
        {
            return false;
        }
        match self.phase {
            MemoPhase::Empty => self.source.is_none() && self.page == 0 && self.access == 0,
            MemoPhase::Associated => self.source.is_some() && self.page == 0 && self.access == 0,
            MemoPhase::Publishing | MemoPhase::Published | MemoPhase::Retiring => {
                self.source.is_some() && self.page != 0 && valid_access(self.access)
            }
            MemoPhase::Failed => {
                self.source.is_some()
                    && if self.page == 0 {
                        self.access == 0
                    } else {
                        valid_access(self.access)
                    }
            }
        }
    }
}

pub(crate) fn valid_access(access: u64) -> bool {
    access & !14 == 0 && access & 10 != 0 && (access & 4 == 0 || access & 2 != 0)
}
