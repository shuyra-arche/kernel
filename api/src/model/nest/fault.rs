//! Next-memory-ABI fault routing values. A route is a Reservation-owned record,
//! never an authorization root. Events describe identities, not data grants.

use super::{MemoGeneration, NestId};
use crate::model::{
    flat::{FlatId, SlotVersion},
    identity::{PendId, TaskId},
    nest::ReservationId,
};
use core::num::NonZeroU64;

/// Maximum retained events per Reservation. Pending events cannot be evicted.
pub const FAULT_CAPACITY: usize = 16;
/// Finite recovery budget in Room.Clock nanoseconds, capped at sixty seconds.
pub const MAX_FAULT_NS: u64 = 60_000_000_000;

/// Non-reusing route generation. Initial disabled routes start at one; replacing
/// or disabling a route advances this value before accepting new events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FaultEpoch(NonZeroU64);
impl FaultEpoch {
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

/// Reservation-wide event identity, distinct from route epochs and Memo generations.
/// Never reset on route replacement; zero is reserved for an initial read cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FaultSequence(NonZeroU64);
impl FaultSequence {
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
pub struct FaultRoute {
    /// Caller-local Sign grant with LISTEN, ROUTE and TRIGGER. The actual caller
    /// owns the registration; no arbitrary service Task can be nominated.
    pub sign: PendId,
    pub timeout_ns: u64,
}
impl FaultRoute {
    pub fn valid(self) -> bool {
        self.sign.get() > 0
            && self.sign.get() <= isize::MAX as usize
            && self.timeout_ns > 0
            && self.timeout_ns <= MAX_FAULT_NS
    }
    /// Calculated once when accepting the fault, never renewed by duplicate
    /// faults, notifications or service reads. Overflow rejects before waiting.
    pub fn deadline(self, accepted_at: u64) -> Option<u64> {
        self.valid().then_some(())?;
        accepted_at.checked_add(self.timeout_ns)
    }
}

/// Validate a service read's deadline against the current Room.Clock. A past
/// deadline is valid but already expired; Infinite and overlong waits reject.
pub fn fault_read_deadline(wait: crate::model::wait::Wait, now: u64) -> Option<u64> {
    match wait {
        crate::model::wait::Wait::Poll => Some(now),
        crate::model::wait::Wait::At(at) if at.saturating_sub(now) <= MAX_FAULT_NS => Some(at),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum FaultPhase {
    Disabled = 0,
    Active = 1,
    Lost = 2,
}
impl FaultPhase {
    pub const fn from_tag(tag: u64) -> Option<Self> {
        match tag {
            0 => Some(Self::Disabled),
            1 => Some(Self::Active),
            2 => Some(Self::Lost),
            _ => None,
        }
    }
}

/// Route status is descriptive. Lost retains its former actor and budget for
/// diagnosis; neither a dead route nor a dead grant can admit recovery actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultStatus {
    pub nest: NestId,
    pub reservation: ReservationId,
    pub epoch: FaultEpoch,
    pub phase: FaultPhase,
    pub handler: Option<TaskId>,
    pub timeout_ns: u64,
    pub last_sequence: u64,
}
impl FaultStatus {
    pub fn valid(self) -> bool {
        self.reservation.get() > 0
            && self.reservation.get() <= isize::MAX as usize
            && match self.phase {
                FaultPhase::Disabled => {
                    self.handler.is_none()
                        && self.timeout_ns == 0
                        && (self.epoch != FaultEpoch::INITIAL || self.last_sequence == 0)
                }
                FaultPhase::Active | FaultPhase::Lost => {
                    self.epoch.get() > 1
                        && self
                            .handler
                            .is_some_and(|task| task.get() > 0 && task.get() <= isize::MAX as usize)
                        && self.timeout_ns > 0
                        && self.timeout_ns <= MAX_FAULT_NS
                }
            }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum FaultCause {
    Fresh = 0,
    Missing = 1,
}
impl FaultCause {
    pub const fn from_tag(tag: u64) -> Option<Self> {
        match tag {
            0 => Some(Self::Fresh),
            1 => Some(Self::Missing),
            _ => None,
        }
    }
}

/// The one attempted access, not a publication access mask or a rights ceiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum FaultAccess {
    Read = 2,
    Write = 4,
    Execute = 8,
}
impl FaultAccess {
    pub const fn from_tag(tag: u64) -> Option<Self> {
        match tag {
            2 => Some(Self::Read),
            4 => Some(Self::Write),
            8 => Some(Self::Execute),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultEvent {
    pub nest: NestId,
    pub reservation: ReservationId,
    pub epoch: FaultEpoch,
    pub sequence: FaultSequence,
    pub memo_page: usize,
    pub generation: MemoGeneration,
    pub flat: FlatId,
    pub slot_page: usize,
    pub version: SlotVersion,
    pub cause: FaultCause,
    pub access: FaultAccess,
}
impl FaultEvent {
    pub fn valid(self) -> bool {
        self.reservation.get() > 0
            && self.reservation.get() <= isize::MAX as usize
            && self.epoch.get() > 1
            && crate::model::cell::pages(self.memo_page, 1).is_some()
            && crate::model::cell::pages(self.slot_page, 1).is_some()
    }
}
