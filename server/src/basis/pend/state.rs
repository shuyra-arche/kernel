//! Pure borrow decisions. Hardware withdrawal must complete before Borrow or Settle
//! is committed; failure retains the previous obligation and never resumes a parent.
use kernel_api::model::modifiable::Modifiable;
use kernel_api::model::pend::{CreationRights, Distributable, Operations};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Active,
    Retiring,
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct State {
    pub(crate) access: Modifiable,
    pub(crate) operations: Operations,
    pub(crate) distributable: Distributable,
    pub(crate) phase: Phase,
    readers: usize,
    writer: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    Borrow(Modifiable),
    Return(Modifiable),
    Narrow(Modifiable),
    Limit {
        access: Modifiable,
        operations: Operations,
        distributable: Distributable,
    },
    Retire,
    Settle,
    Cancel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Denied,
    Busy,
    Dead,
}
impl State {
    pub(crate) const fn new(access: Modifiable) -> Self {
        Self {
            access,
            operations: Operations::None(CreationRights::empty()),
            distributable: Distributable::No,
            phase: Phase::Active,
            readers: 0,
            writer: false,
        }
    }
    pub(crate) fn authorized(
        access: Modifiable,
        operations: Operations,
        distributable: Distributable,
    ) -> Self {
        Self {
            operations,
            distributable,
            ..Self::new(access)
        }
    }
    pub(crate) fn permits(self, operations: Operations) -> bool {
        self.usable() && self.operations.contains(operations)
    }
    pub(crate) fn usable(self) -> bool {
        self.phase == Phase::Active
            && !self.writer
            && (self.access == Modifiable::No || self.readers == 0)
    }
    pub(crate) fn transition(self, event: Event) -> Result<Self, Failure> {
        let mut next = self;
        match event {
            Event::Retire => {
                if next.phase != Phase::Dead {
                    next.phase = Phase::Retiring;
                }
            }
            Event::Settle | Event::Cancel => {
                match (event, next.phase) {
                    (Event::Settle, Phase::Retiring | Phase::Dead)
                    | (Event::Cancel, Phase::Active) => {}
                    _ => return Err(Failure::Denied),
                }
                if next.readers != 0 || next.writer {
                    return Err(Failure::Busy);
                }
                next.phase = Phase::Dead;
            }
            Event::Return(access) => match access {
                Modifiable::Yes if next.writer => next.writer = false,
                Modifiable::No if next.readers != 0 => next.readers -= 1,
                _ => return Err(Failure::Denied),
            },
            Event::Borrow(access) => {
                if next.phase != Phase::Active {
                    return Err(Failure::Dead);
                }
                if !next.access.allows(access) {
                    return Err(Failure::Denied);
                }
                if next.writer || (access == Modifiable::Yes && next.readers != 0) {
                    return Err(Failure::Busy);
                }
                match access {
                    Modifiable::Yes => next.writer = true,
                    Modifiable::No => {
                        next.readers = next.readers.checked_add(1).ok_or(Failure::Busy)?
                    }
                }
            }
            Event::Limit {
                access,
                operations,
                distributable,
            } => {
                if !next.operations.contains(operations)
                    || (next.distributable == Distributable::No
                        && distributable == Distributable::Yes)
                {
                    return Err(Failure::Denied);
                }
                next = next.transition(Event::Narrow(access))?;
                next.operations = operations;
                next.distributable = distributable;
            }
            Event::Narrow(access) => {
                if next.phase != Phase::Active {
                    return Err(Failure::Dead);
                }
                if !next.access.allows(access) {
                    return Err(Failure::Denied);
                }
                if next.readers != 0 || next.writer {
                    return Err(Failure::Busy);
                }
                next.access = access;
            }
        }
        Ok(next)
    }
}
