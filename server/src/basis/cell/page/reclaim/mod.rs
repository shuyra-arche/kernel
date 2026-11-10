//! 物理归还的纯判定与原子提交；引用消失不自动清偿硬件访问责任。

pub(in super::super) mod change;
mod retention;

pub(crate) use change::{Change, Freeze};
pub(crate) use retention::Retention;

use super::Exposure;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

const QUARANTINED: usize = 1 << (usize::BITS - 1);
const EXTERNAL_WRITER: usize = 1 << (usize::BITS - 2);
const CHANGING: usize = 1 << (usize::BITS - 3);
const COUNTER_BITS: u32 = (usize::BITS - 3) / 4;
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
}
use kernel_api::model::modifiable::Modifiable;

#[derive(Clone, Copy)]
enum Event {
    Register {
        kind: Kind,
        access: Modifiable,
        admission: Admission,
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
    },
    Abandon,
}

#[derive(Clone, Copy)]
struct State {
    cpu_readers: usize,
    cpu_writers: usize,
    external: usize,
    freezes: usize,
    quarantined: bool,
    external_writer: bool,
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
                admission: Admission::Changing,
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
            } => {
                let writable = access == Modifiable::Yes;
                let permitted_freezes = usize::from(matches!(admission, Admission::Changing));
                if writable && self.freezes > permitted_freezes {
                    return Err(RetainError::Busy);
                }
                // CPU mappings share coherent RAM. Preserve the exclusion of
                // writable external access and of CPU writes during external use.
                let conflicts = match (kind, writable) {
                    (Kind::Cpu, false) => self.external_writer,
                    (Kind::Cpu, true) => self.external != 0,
                    (Kind::External, false) => self.writers() != 0,
                    (Kind::External, true) => {
                        self.external != 0 || self.cpu_readers != 0 || self.cpu_writers != 0
                    }
                };
                if conflicts {
                    return Err(RetainError::Borrowed);
                }
                let mut next = self;
                let uses = next.uses(kind, writable);
                if *uses == MAX_USES {
                    return Err(RetainError::Exhausted);
                }
                *uses += 1;
                if matches!(kind, Kind::External) {
                    next.external_writer = writable;
                }
                Ok(next)
            }
            Event::Discharge { kind, access } => {
                let writable = access == Modifiable::Yes;
                let mut next = self;
                if matches!(kind, Kind::External) {
                    if writable != self.external_writer {
                        return Err(RetainError::Unbalanced);
                    }
                    next.external_writer = false;
                }
                let uses = next.uses(kind, writable);
                *uses = uses.checked_sub(1).ok_or(RetainError::Unbalanced)?;
                Ok(next)
            }
            Event::Abandon => Ok(Self {
                quarantined: true,
                ..self
            }),
        }
    }

    fn writers(self) -> usize {
        self.cpu_writers + usize::from(self.external_writer)
    }

    fn uses(&mut self, kind: Kind, writable: bool) -> &mut usize {
        match (kind, writable) {
            (Kind::Cpu, false) => &mut self.cpu_readers,
            (Kind::Cpu, true) => &mut self.cpu_writers,
            (Kind::External, _) => &mut self.external,
        }
    }

    fn decode(raw: usize) -> Self {
        Self {
            cpu_readers: raw & MAX_USES,
            cpu_writers: (raw >> COUNTER_BITS) & MAX_USES,
            external: (raw >> (2 * COUNTER_BITS)) & MAX_USES,
            freezes: (raw >> (3 * COUNTER_BITS)) & MAX_USES,
            quarantined: raw & QUARANTINED != 0,
            external_writer: raw & EXTERNAL_WRITER != 0,
            changing: raw & CHANGING != 0,
        }
    }

    fn encode(self) -> usize {
        self.cpu_readers
            | (self.cpu_writers << COUNTER_BITS)
            | (self.external << (2 * COUNTER_BITS))
            | (self.freezes << (3 * COUNTER_BITS))
            | if self.quarantined { QUARANTINED } else { 0 }
            | if self.external_writer {
                EXTERNAL_WRITER
            } else {
                0
            }
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

    pub(super) fn register(&self, kind: Kind, modifiable: bool) -> Result<(), RetainError> {
        self.apply(Event::Register {
            kind,
            access: mode(modifiable),
            admission: Admission::Ordinary,
        })
    }
    pub(super) fn register_during(&self, kind: Kind, modifiable: bool) -> Result<(), RetainError> {
        self.apply(Event::Register {
            kind,
            access: mode(modifiable),
            admission: Admission::Changing,
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
    pub(super) fn discharge(&self, kind: Kind, modifiable: bool) {
        self.apply(Event::Discharge {
            kind,
            access: mode(modifiable),
        })
        .expect("a retention discharges exactly one registered use");
    }
    pub(super) fn abandon(&self) {
        self.apply(Event::Abandon)
            .expect("quarantine never rejects");
    }
    pub(super) fn allows_return(&self) -> bool {
        self.0.load(Ordering::Acquire) == 0
    }

    pub(super) fn exposure(&self) -> Exposure {
        if State::decode(self.0.load(Ordering::Acquire)).external == 0 {
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
