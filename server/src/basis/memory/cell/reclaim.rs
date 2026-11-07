//! 物理归还的纯判定与原子提交；引用消失不自动清偿硬件访问责任。

use super::Exposure;
use core::sync::atomic::{AtomicUsize, Ordering};

const QUARANTINED: usize = 1 << (usize::BITS - 1);
const MODIFIABLE: usize = 1 << (usize::BITS - 2);
const COUNTER_BITS: u32 = (usize::BITS - 2) / 2;
const MAX_USES: usize = (1 << COUNTER_BITS) - 1;

#[derive(Debug, Clone, Copy)]
pub(super) enum Kind {
    Cpu,
    Device,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetainError {
    Quarantined,
    Exhausted,
    Borrowed,
    Unbalanced,
}

#[derive(Clone, Copy)]
enum Event {
    Register(Kind, bool),
    Discharge(Kind, bool),
    Abandon,
}

#[derive(Clone, Copy)]
struct State {
    cpu: usize,
    device: usize,
    quarantined: bool,
    modifiable: bool,
}

impl State {
    fn transition(self, event: Event) -> Result<Self, RetainError> {
        match event {
            Event::Register(..) if self.quarantined => Err(RetainError::Quarantined),
            Event::Register(kind, modifiable) => {
                if self.modifiable || (modifiable && (self.cpu != 0 || self.device != 0)) {
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
            Kind::Device => self.device,
        }
    }

    fn with_uses(self, kind: Kind, uses: usize) -> Self {
        match kind {
            Kind::Cpu => Self { cpu: uses, ..self },
            Kind::Device => Self {
                device: uses,
                ..self
            },
        }
    }

    fn decode(raw: usize) -> Self {
        Self {
            cpu: raw & MAX_USES,
            device: (raw >> COUNTER_BITS) & MAX_USES,
            quarantined: raw & QUARANTINED != 0,
            modifiable: raw & MODIFIABLE != 0,
        }
    }

    fn encode(self) -> usize {
        self.cpu
            | (self.device << COUNTER_BITS)
            | if self.quarantined { QUARANTINED } else { 0 }
            | if self.modifiable { MODIFIABLE } else { 0 }
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
        self.apply(Event::Register(kind, modifiable))
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
        if State::decode(self.0.load(Ordering::Acquire)).device == 0 {
            Exposure::Internal
        } else {
            Exposure::External
        }
    }
}
