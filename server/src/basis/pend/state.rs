//! Pure borrow decisions. Hardware withdrawal must complete before Borrow or Settle
//! is committed; failure retains the previous obligation and never resumes a parent.
use kernel_api::model::modifiable::Modifiable;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Active,
    Retiring,
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct State {
    pub(crate) access: Modifiable,
    pub(crate) phase: Phase,
    readers: usize,
    writer: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    Borrow(Modifiable),
    Return(Modifiable),
    Narrow(Modifiable),
    Retire,
    Settle,
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
            phase: Phase::Active,
            readers: 0,
            writer: false,
        }
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
            Event::Settle => {
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
