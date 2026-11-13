//! One Task owns its reservation and accounting; COMMIT serializes dependency
//! changes with queue selection. Effective priority is a derived graph result.
pub(crate) mod account;
use super::Task;
use account::{Account, Contract};
use alloc::sync::Weak;

pub(crate) struct Schedule {
    state: State,
}

pub(crate) enum Dependency {
    Keep,
    Await(Weak<Task>),
    Clear,
}

enum State {
    Ordinary {
        cursor: usize,
    },
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
        Self {
            state: State::Ordinary { cursor: 0 },
        }
    }
    pub(crate) fn ordinary(&self) -> bool {
        matches!(self.state, State::Ordinary { .. })
    }
    pub(crate) fn preparing(&self) -> bool {
        matches!(self.state, State::Preparing)
    }
    pub(crate) fn prepare(&mut self) -> bool {
        if !self.ordinary() {
            return false;
        }
        self.state = State::Preparing;
        true
    }
    pub(crate) fn cancel(&mut self) {
        if self.preparing() {
            *self = Self::new();
        }
    }
    pub(crate) fn admit(&mut self, contract: Contract) -> bool {
        if !self.preparing() {
            return false;
        }
        self.state = State::Fixed {
            account: Account::new(contract.budget, contract.period),
            effective: contract.spec.priority as u8,
            contract,
            dependency: None,
        };
        true
    }
    pub(crate) fn release(&mut self) -> Option<Contract> {
        let contract = self.contract()?;
        *self = Self::new();
        Some(contract)
    }
    /// COMMIT retains the admitted contract through graph calculation/publication.
    pub(crate) fn response(&mut self, analyzed: Contract) {
        if let State::Fixed { contract, .. } = &mut self.state
            && contract.spec == analyzed.spec
        {
            contract.response = analyzed.response;
        }
    }
    pub(crate) fn dependency(&self) -> Option<alloc::sync::Arc<Task>> {
        match &self.state {
            State::Fixed { dependency, .. } => dependency.as_ref().and_then(Weak::upgrade),
            _ => None,
        }
    }
    /// A validated graph supplies the effective priority and optional edge change.
    /// The owner still enforces the admitted priority bounds before publication.
    pub(crate) fn donate(&mut self, effective: u8, edge: Dependency) {
        let State::Fixed {
            contract,
            effective: current,
            dependency,
            ..
        } = &mut self.state
        else {
            panic!("donation requires an admitted Task");
        };
        assert!(
            effective >= contract.spec.priority as u8 && effective <= contract.spec.ceiling as u8
        );
        *current = effective;
        match edge {
            Dependency::Keep => {}
            Dependency::Await(server) => *dependency = Some(server),
            Dependency::Clear => *dependency = None,
        }
    }
    pub(crate) fn advance(&mut self, now: u64) {
        if let State::Fixed { account, .. } = &mut self.state {
            account.advance(now);
        }
    }
    pub(crate) fn observation(&self) -> Option<(Contract, Account, u8)> {
        match &self.state {
            State::Fixed {
                contract,
                account,
                effective,
                ..
            } => Some((*contract, *account, *effective)),
            _ => None,
        }
    }
    /// Wake placement belongs to this Task: unrelated wakeups must not phase
    /// lock a periodic worker to one residue of a shared global cursor.
    pub(crate) fn placement(&mut self) -> usize {
        match &mut self.state {
            State::Ordinary { cursor } => {
                let next = *cursor;
                *cursor = cursor.wrapping_add(1);
                next
            }
            State::Fixed { contract, .. } => contract.spec.hart as usize,
            State::Preparing => unreachable!("preparing Task cannot be placed"),
        }
    }
    pub(crate) fn contract(&self) -> Option<Contract> {
        match &self.state {
            State::Ordinary { .. } | State::Preparing => None,
            State::Fixed { contract, .. } => Some(*contract),
        }
    }
    pub(crate) fn priority(&self) -> u8 {
        match &self.state {
            State::Ordinary { .. } | State::Preparing => 0,
            State::Fixed { effective, .. } => *effective,
        }
    }
    pub(crate) fn available(&self) -> bool {
        match &self.state {
            State::Ordinary { .. } => true,
            State::Preparing => false,
            State::Fixed { account, .. } => account.remaining() != 0,
        }
    }
    pub(crate) fn deadline(&self, now: u64) -> u64 {
        match &self.state {
            State::Ordinary { .. } | State::Preparing => u64::MAX,
            State::Fixed { account, .. } => account.deadline(now),
        }
    }
    pub(crate) fn enter(&mut self, now: u64) {
        if let State::Fixed { account, .. } = &mut self.state {
            account.enter(now);
        }
    }
    pub(crate) fn leave(&mut self, now: u64) {
        if let State::Fixed { account, .. } = &mut self.state {
            account.leave(now);
        }
    }
}
