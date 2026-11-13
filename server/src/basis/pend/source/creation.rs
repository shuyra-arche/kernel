//! One-use source adoption. The claim owns only qualification rollback; exact
//! physical retirement always remains with the existing publication owners.
use core::sync::atomic::{AtomicU8, Ordering};
use kernel_api::abi::call::pend::PendFail;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum CreationState {
    Available,
    Claimed,
    Consumed,
    Expired,
}
pub(super) struct Creation {
    state: AtomicU8,
}
impl Creation {
    pub(super) fn new() -> Self {
        Self {
            state: AtomicU8::new(CreationState::Available as u8),
        }
    }
    pub(super) fn claim(&self) -> Result<Claim<'_>, PendFail> {
        match self.state.compare_exchange(
            CreationState::Available as u8,
            CreationState::Claimed as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(Claim {
                creation: self,
                committed: false,
            }),
            Err(value) if value == CreationState::Claimed as u8 => Err(PendFail::Busy),
            Err(_) => Err(PendFail::Denied),
        }
    }
    pub(super) fn claimed(&self) -> bool {
        self.state.load(Ordering::Acquire) == CreationState::Claimed as u8
    }
    pub(super) fn expire(&self) {
        // Losing the final ordinary hold invalidates even a concurrent claim.
        // Neither rollback nor a late commit can resurrect that qualification.
        self.state
            .store(CreationState::Expired as u8, Ordering::Release);
    }
}
/// Owns only the unused qualification, never hardware withdrawal obligations.
#[must_use]
pub(crate) struct Claim<'a> {
    creation: &'a Creation,
    committed: bool,
}
impl Claim<'_> {
    pub(crate) fn commit(mut self) {
        self.creation
            .state
            .compare_exchange(
                CreationState::Claimed as u8,
                CreationState::Consumed as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .expect("qualification remains claimed through authorized adoption");
        self.committed = true;
    }
}
impl Drop for Claim<'_> {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.creation.state.compare_exchange(
                CreationState::Claimed as u8,
                CreationState::Available as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
}
