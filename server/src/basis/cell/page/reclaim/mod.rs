//! 物理归还的纯判定与原子提交；引用消失不自动清偿硬件访问责任。

pub(in super::super) mod change;
mod retention;

pub(crate) use change::{Change, Freeze};
pub(crate) use retention::Retention;

use super::Exposure;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

const QUARANTINED: usize = 1 << (usize::BITS - 1);
const MODIFIABLE: usize = 1 << (usize::BITS - 2);
const CHANGING: usize = 1 << (usize::BITS - 3);
const COUNTER_BITS: u32 = (usize::BITS - 3) / 2;
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
enum Event {
    Register(Kind, bool, bool),
    Begin,
    End,
    Discharge(Kind, bool),
    Abandon,
}

#[derive(Clone, Copy)]
struct State {
    cpu: usize,
    external: usize,
    quarantined: bool,
    modifiable: bool,
    changing: bool,
}

impl State {
    fn transition(self, event: Event) -> Result<Self, RetainError> {
        match event {
            Event::Register(..) if self.quarantined => Err(RetainError::Quarantined),
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
            Event::Register(_, _, privileged) if self.changing && !privileged => {
                Err(RetainError::Busy)
            }
            Event::Register(_, _, true) if !self.changing => Err(RetainError::Unbalanced),
            Event::Register(kind, modifiable, _) => {
                if self.modifiable || (modifiable && (self.cpu != 0 || self.external != 0)) {
                    return Err(RetainError::Borrowed);
                }
                let uses = self.uses(kind);
                if uses == MAX_USES {
                    return Err(RetainError::Exhausted);
                }
                Ok(Self {
                    modifiable,
                    ..self.with_uses(kind, uses + 1)
                })
            }
            Event::Discharge(kind, modifiable) => {
                if modifiable != self.modifiable {
                    return Err(RetainError::Unbalanced);
                }
                let uses = self
                    .uses(kind)
                    .checked_sub(1)
                    .ok_or(RetainError::Unbalanced)?;
                Ok(Self {
                    modifiable: false,
                    ..self.with_uses(kind, uses)
                })
            }
            Event::Abandon => Ok(Self {
                quarantined: true,
                ..self
            }),
        }
    }

    fn uses(self, kind: Kind) -> usize {
        match kind {
            Kind::Cpu => self.cpu,
            Kind::External => self.external,
        }
    }

    fn with_uses(self, kind: Kind, uses: usize) -> Self {
        match kind {
            Kind::Cpu => Self { cpu: uses, ..self },
            Kind::External => Self {
                external: uses,
                ..self
            },
        }
    }

    fn decode(raw: usize) -> Self {
        Self {
            cpu: raw & MAX_USES,
            external: (raw >> COUNTER_BITS) & MAX_USES,
            quarantined: raw & QUARANTINED != 0,
            modifiable: raw & MODIFIABLE != 0,
            changing: raw & CHANGING != 0,
        }
    }

    fn encode(self) -> usize {
        self.cpu
            | (self.external << COUNTER_BITS)
            | if self.quarantined { QUARANTINED } else { 0 }
            | if self.modifiable { MODIFIABLE } else { 0 }
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
        self.apply(Event::Register(kind, modifiable, false))
    }
    pub(super) fn register_during(&self, kind: Kind, modifiable: bool) -> Result<(), RetainError> {
        self.apply(Event::Register(kind, modifiable, true))
    }
    pub(super) fn begin(&self) -> Result<(), RetainError> {
        self.apply(Event::Begin)
    }
    pub(super) fn end(&self) {
        self.apply(Event::End).expect("live change");
    }
    pub(super) fn frozen(&self) -> bool {
        let state = State::decode(self.0.load(Ordering::Acquire));
        state.changing && !state.modifiable && !state.quarantined
    }
    pub(super) fn is_changing(&self) -> bool {
        State::decode(self.0.load(Ordering::Acquire)).changing
    }
    pub(super) fn discharge(&self, kind: Kind, modifiable: bool) {
        self.apply(Event::Discharge(kind, modifiable))
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
