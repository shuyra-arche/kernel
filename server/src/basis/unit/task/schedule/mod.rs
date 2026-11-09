//! One Task owns its reservation and accounting; COMMIT serializes dependency
//! changes with queue selection. Effective priority is a derived graph result.
pub(crate) mod account;
use super::Task;
use account::{Account, Contract};
use alloc::sync::Weak;

pub(crate) enum Schedule {
    Ordinary,
    /// The stopped Task is leased to fallible resource preparation.
    Preparing,
    Fixed {
        contract: Contract,
        account: Account,
        effective: u8,
        dependency: Option<Weak<Task>>,
    },
}
impl Schedule {
    pub(crate) const fn new() -> Self {
        Self::Ordinary
    }
    pub(crate) fn contract(&self) -> Option<Contract> {
        match self {
            Self::Ordinary | Self::Preparing => None,
            Self::Fixed { contract, .. } => Some(*contract),
        }
    }
    pub(crate) fn priority(&self) -> u8 {
        match self {
            Self::Ordinary | Self::Preparing => 0,
            Self::Fixed { effective, .. } => *effective,
        }
    }
    pub(crate) fn available(&mut self, now: u64) -> bool {
        match self {
            Self::Ordinary => true,
            Self::Preparing => false,
            Self::Fixed { account, .. } => account.remaining(now) != 0,
        }
    }
    pub(crate) fn deadline(&mut self, now: u64) -> u64 {
        match self {
            Self::Ordinary | Self::Preparing => u64::MAX,
            Self::Fixed { account, .. } => account.deadline(now),
        }
    }
    pub(crate) fn enter(&mut self, now: u64) {
        if let Self::Fixed { account, .. } = self {
            account.enter(now);
        }
    }
    pub(crate) fn leave(&mut self, now: u64) {
        if let Self::Fixed { account, .. } = self {
            account.leave(now);
        }
    }
    pub(crate) fn charge(&mut self, now: u64) {
        if let Self::Fixed { account, .. } = self {
            account.charge(now);
        }
    }
}
