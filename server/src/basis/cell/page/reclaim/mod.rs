//! 物理归还的纯判定与原子提交；引用消失不自动清偿硬件访问责任。

pub(in super::super) mod change;
pub(super) mod group;
mod retention;
pub(crate) mod statistics;

pub(crate) use change::{Block, Change, Freeze, Maintenance};
pub(crate) use retention::Retention;

use super::Exposure;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

const QUARANTINED: usize = 1 << (usize::BITS - 1);
const CHANGING: usize = 1 << (usize::BITS - 2);
const COUNTER_BITS: u32 = (usize::BITS - 2) / 6;
const MAX_USES: usize = (1 << COUNTER_BITS) - 1;

#[derive(Debug, Clone, Copy)]
pub(super) enum Kind {
    Cpu,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetainError {
    Quarantined,
    Exhausted,
    Borrowed,
    Busy,
    Unbalanced,
}

#[derive(Clone, Copy)]
enum Admission {
    Ordinary,
    Changing,
    Maintenance,
}
use kernel_api::model::modifiable::Modifiable;

#[derive(Clone, Copy)]
enum Event {
    Register {
        kind: Kind,
        access: Modifiable,
        admission: Admission,
        qualified: bool,
    },
    Begin,
    End,
    Freeze {
        retracts: Option<bool>,
    },
    Thaw,
    Discharge {
        kind: Kind,
        access: Modifiable,
        maintenance: bool,
    },
    Abandon,
}

#[derive(Clone, Copy)]
struct State {
    cpu_readers: usize,
    cpu_writers: usize,
    external_readers: usize,
    external_writers: usize,
    freezes: usize,
    maintenance: usize,
    quarantined: bool,
    changing: bool,
}

impl State {
    fn transition(self, event: Event) -> Result<Self, RetainError> {
        match event {
            Event::Register { .. } if self.quarantined => Err(RetainError::Quarantined),
            Event::Begin if self.quarantined => Err(RetainError::Quarantined),
            Event::Begin if self.changing => Err(RetainError::Busy),
            Event::Begin => Ok(Self {
                changing: true,
                ..self
            }),
            Event::End if !self.changing => Err(RetainError::Unbalanced),
            Event::End => Ok(Self {
                changing: false,
                ..self
            }),
            Event::Register {
                admission: Admission::Ordinary,
                ..
            } if self.changing => Err(RetainError::Busy),
            Event::Register {
                admission: Admission::Changing | Admission::Maintenance,
                ..
            } if !self.changing => Err(RetainError::Unbalanced),
            Event::Freeze { retracts } => {
                if self.quarantined {
                    return Err(RetainError::Quarantined);
                }
                if retracts.is_some_and(|writable| self.writers() > usize::from(writable)) {
                    return Err(RetainError::Busy);
                }
                if self.freezes == MAX_USES {
                    return Err(RetainError::Exhausted);
                }
                Ok(Self {
                    freezes: self.freezes + 1,
                    ..self
                })
            }
            Event::Thaw => Ok(Self {
                freezes: self.freezes.checked_sub(1).ok_or(RetainError::Unbalanced)?,
                ..self
            }),
            Event::Register {
                kind,
                access,
                admission,
                qualified,
            } => {
                let writable = access == Modifiable::Yes;
                let permitted_freezes = usize::from(!matches!(admission, Admission::Ordinary));
                if writable && self.freezes > permitted_freezes {
                    return Err(RetainError::Busy);
                }
                // Unknown/legacy participants keep conservative exclusion. Only
                // the Page membership gate may supply a whole-group qualification.
                let conflicts = match (kind, writable) {
                    (Kind::Cpu, false) => self.external_writers != 0,
                    (Kind::Cpu, true) => self.external_readers != 0 || self.external_writers != 0,
                    (Kind::External, false) => self.writers() != 0,
                    (Kind::External, true) => {
                        self.external_readers != 0
                            || self.external_writers != 0
                            || self.cpu_readers != 0
                            || self.cpu_writers != 0
                    }
                };
                if conflicts && !qualified {
                    return Err(RetainError::Borrowed);
                }
                let mut next = self;
                if matches!(admission, Admission::Maintenance) {
                    if next.maintenance == MAX_USES {
                        return Err(RetainError::Exhausted);
                    }
                    next.maintenance += 1;
                }
                let uses = next.uses(kind, writable);
                if *uses == MAX_USES {
                    return Err(RetainError::Exhausted);
                }
                *uses += 1;
                Ok(next)
            }
            Event::Discharge {
                kind,
                access,
                maintenance,
            } => {
                let writable = access == Modifiable::Yes;
                let mut next = self;
                let uses = next.uses(kind, writable);
                *uses = uses.checked_sub(1).ok_or(RetainError::Unbalanced)?;
                if maintenance {
                    next.maintenance = next
                        .maintenance
                        .checked_sub(1)
                        .ok_or(RetainError::Unbalanced)?;
                }
                Ok(next)
            }
            Event::Abandon => Ok(Self {
                quarantined: true,
                ..self
            }),
        }
    }

    fn writers(self) -> usize {
        self.cpu_writers + self.external_writers
    }

    fn uses(&mut self, kind: Kind, writable: bool) -> &mut usize {
        match (kind, writable) {
            (Kind::Cpu, false) => &mut self.cpu_readers,
            (Kind::Cpu, true) => &mut self.cpu_writers,
            (Kind::External, false) => &mut self.external_readers,
            (Kind::External, true) => &mut self.external_writers,
        }
    }

    fn decode(raw: usize) -> Self {
        Self {
            cpu_readers: raw & MAX_USES,
            cpu_writers: (raw >> COUNTER_BITS) & MAX_USES,
            external_readers: (raw >> (2 * COUNTER_BITS)) & MAX_USES,
            external_writers: (raw >> (3 * COUNTER_BITS)) & MAX_USES,
            freezes: (raw >> (4 * COUNTER_BITS)) & MAX_USES,
            maintenance: (raw >> (5 * COUNTER_BITS)) & MAX_USES,
            quarantined: raw & QUARANTINED != 0,
            changing: raw & CHANGING != 0,
        }
    }

    fn encode(self) -> usize {
        self.cpu_readers
            | (self.cpu_writers << COUNTER_BITS)
            | (self.external_readers << (2 * COUNTER_BITS))
            | (self.external_writers << (3 * COUNTER_BITS))
            | (self.freezes << (4 * COUNTER_BITS))
            | (self.maintenance << (5 * COUNTER_BITS))
            | if self.quarantined { QUARANTINED } else { 0 }
            | if self.changing { CHANGING } else { 0 }
    }
}

#[derive(Debug)]
pub(super) struct Reclaim(AtomicUsize);

impl Reclaim {
    pub(super) const fn new() -> Self {
        Self(AtomicUsize::new(0))
    }

    fn apply(&self, event: Event) -> Result<(), RetainError> {
        let mut before = self.0.load(Ordering::Acquire);
        loop {
            let after = State::decode(before).transition(event)?.encode();
            match self
                .0
                .compare_exchange_weak(before, after, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return Ok(()),
                Err(current) => before = current,
            }
        }
    }

    pub(super) fn register(
        &self,
        kind: Kind,
        modifiable: bool,
        qualified: bool,
    ) -> Result<(), RetainError> {
        self.apply(Event::Register {
            kind,
            access: mode(modifiable),
            admission: Admission::Ordinary,
            qualified,
        })
    }
    pub(super) fn register_during(
        &self,
        kind: Kind,
        modifiable: bool,
        maintenance: bool,
        qualified: bool,
    ) -> Result<(), RetainError> {
        self.apply(Event::Register {
            kind,
            access: mode(modifiable),
            qualified,
            admission: if maintenance {
                Admission::Maintenance
            } else {
                Admission::Changing
            },
        })
    }
    pub(super) fn begin(&self) -> Result<(), RetainError> {
        self.apply(Event::Begin)
    }
    pub(super) fn end(&self) {
        self.apply(Event::End).expect("live change");
    }
    pub(super) fn freeze(&self, retracts: Option<bool>) -> Result<(), RetainError> {
        self.apply(Event::Freeze { retracts })
    }
    pub(super) fn thaw(&self) {
        self.apply(Event::Thaw).expect("live freeze");
    }
    pub(super) fn frozen(&self) -> bool {
        let state = State::decode(self.0.load(Ordering::Acquire));
        state.changing && state.writers() == 0 && !state.quarantined
    }
    pub(super) fn is_changing(&self) -> bool {
        State::decode(self.0.load(Ordering::Acquire)).changing
    }
    pub(super) fn discharge(&self, kind: Kind, modifiable: bool, maintenance: bool) {
        self.apply(Event::Discharge {
            kind,
            maintenance,
            access: mode(modifiable),
        })
        .expect("a retention discharges exactly one registered use");
    }
    pub(super) fn abandon(&self) {
        self.apply(Event::Abandon)
            .expect("quarantine never rejects");
    }
    pub(super) fn hardware_idle(&self) -> bool {
        let state = State::decode(self.0.load(Ordering::Acquire));
        !state.quarantined
            && state.cpu_readers == 0
            && state.cpu_writers == 0
            && state.external_readers == 0
            && state.external_writers == 0
    }
    pub(super) fn maintenance(&self) -> u64 {
        State::decode(self.0.load(Ordering::Acquire)).maintenance as u64
    }
    pub(super) fn quarantined(&self) -> bool {
        State::decode(self.0.load(Ordering::Acquire)).quarantined
    }
    pub(super) fn obligations(&self) -> u64 {
        let state = State::decode(self.0.load(Ordering::Acquire));
        (state.cpu_readers + state.cpu_writers + state.external_readers + state.external_writers)
            as u64
    }
    pub(super) fn allows_return(&self) -> bool {
        self.0.load(Ordering::Acquire) == 0
    }

    pub(super) fn exposure(&self) -> Exposure {
        if {
            let state = State::decode(self.0.load(Ordering::Acquire));
            state.external_readers == 0 && state.external_writers == 0
        } {
            Exposure::Internal
        } else {
            Exposure::External
        }
    }
}

fn mode(writable: bool) -> Modifiable {
    if writable {
        Modifiable::Yes
    } else {
        Modifiable::No
    }
}
