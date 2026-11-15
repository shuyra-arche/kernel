//! One preparer at a time, before any Hart may enter the final barrier.
use core::sync::atomic::{AtomicU8, Ordering};
const IDLE: u8 = 0;
const RUNNING: u8 = 1;
const READY: u8 = 2;
const FAILED: u8 = 3;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unsettled {
    Busy,
    Failed,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Claim {
    Prepare,
    Wait,
    Ready,
    Failed,
}
pub(super) struct Preparation(AtomicU8);
impl Preparation {
    pub(super) const fn new() -> Self {
        Self(AtomicU8::new(IDLE))
    }
    pub(super) fn claim(&self) -> Claim {
        match self
            .0
            .compare_exchange(IDLE, RUNNING, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => Claim::Prepare,
            Err(RUNNING) => Claim::Wait,
            Err(READY) => Claim::Ready,
            Err(FAILED) => Claim::Failed,
            _ => unreachable!(),
        }
    }
    pub(super) fn complete(&self, result: Result<(), Unsettled>) -> bool {
        let next = match result {
            Ok(()) => READY,
            Err(Unsettled::Busy) => IDLE,
            Err(Unsettled::Failed) => FAILED,
        };
        self.0
            .compare_exchange(RUNNING, next, Ordering::Release, Ordering::Relaxed)
            .expect("only the claimed preparer may publish completion");
        next == READY
    }
}
