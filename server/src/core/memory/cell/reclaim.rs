//! 物理归还的纯判定与原子提交；引用消失不自动清偿硬件访问责任。

use core::sync::atomic::{AtomicUsize, Ordering};

const QUARANTINED: usize = 1 << (usize::BITS - 1);
const MAX_USES: usize = QUARANTINED - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetainError {
    Quarantined,
    Exhausted,
    Unbalanced,
}

#[derive(Clone, Copy)]
enum Event {
    Register,
    Discharge,
    Abandon,
}

#[derive(Clone, Copy)]
struct State {
    uses: usize,
    quarantined: bool,
}

impl State {
    fn transition(self, event: Event) -> Result<Self, RetainError> {
        match event {
            Event::Register if self.quarantined => Err(RetainError::Quarantined),
            Event::Register if self.uses == MAX_USES => Err(RetainError::Exhausted),
            Event::Register => Ok(Self {
                uses: self.uses + 1,
                ..self
            }),
            Event::Discharge if self.uses == 0 => Err(RetainError::Unbalanced),
            Event::Discharge => Ok(Self {
                uses: self.uses - 1,
                ..self
            }),
            Event::Abandon => Ok(Self {
                quarantined: true,
                ..self
            }),
        }
    }

    fn decode(raw: usize) -> Self {
        Self {
            uses: raw & MAX_USES,
            quarantined: raw & QUARANTINED != 0,
        }
    }

    fn encode(self) -> usize {
        self.uses | if self.quarantined { QUARANTINED } else { 0 }
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

    pub(super) fn register(&self) -> Result<(), RetainError> {
        self.apply(Event::Register)
    }
    pub(super) fn discharge(&self) {
        self.apply(Event::Discharge)
            .expect("a retention discharges exactly one registered use");
    }
    pub(super) fn abandon(&self) {
        self.apply(Event::Abandon)
            .expect("quarantine never rejects");
    }
    pub(super) fn allows_return(&self) -> bool {
        self.0.load(Ordering::Acquire) == 0
    }
}
