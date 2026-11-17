use crate::basis::{self, unit::task::Task};
use alloc::sync::Arc;
use kernel_api::abi::memory::{
    self, Failure, Outcome, Reply,
    journal::{Accepted, RequestKey},
};

/// Exactly one owner can finish an accepted invocation. The enum excludes a
/// detached receipt that still carries an execution permit or missing value.
enum State {
    Ready(Accepted),
    Started {
        accepted: Accepted,
        value: memory::Value,
    },
    Committed(Reply),
    Deferred,
}

pub(crate) struct Receipt<'a> {
    actor: &'a Arc<Task>,
    request: RequestKey,
    state: State,
}
impl<'a> Receipt<'a> {
    pub(super) fn new(actor: &'a Arc<Task>, request: RequestKey, accepted: Accepted) -> Self {
        Self {
            actor,
            request,
            state: State::Ready(accepted),
        }
    }
    pub(crate) fn committed(&mut self, value: memory::Value) {
        let accepted = match core::mem::replace(&mut self.state, State::Deferred) {
            State::Ready(accepted) | State::Started { accepted, .. } => accepted,
            _ => panic!("completion already transferred"),
        };
        self.state = State::Committed(self.record(&accepted, Outcome::Committed(value)));
    }
    pub(crate) fn start(&mut self, value: memory::Value) {
        let accepted = match core::mem::replace(&mut self.state, State::Deferred) {
            State::Ready(accepted) | State::Started { accepted, .. } => accepted,
            _ => panic!("completion already transferred"),
        };
        self.record(&accepted, Outcome::Pending(value));
        self.state = State::Started { accepted, value };
    }
    pub(crate) fn started(&self) -> Option<memory::Value> {
        match self.state {
            State::Started { value, .. } => Some(value),
            _ => None,
        }
    }
    pub(crate) fn defer(&mut self) -> super::Completion {
        let State::Started { accepted, value } =
            core::mem::replace(&mut self.state, State::Deferred)
        else {
            panic!("only accepted work can transfer completion")
        };
        super::Completion::new(self.actor, self.request, accepted, value)
    }
    fn record(&self, accepted: &Accepted, outcome: Outcome) -> Reply {
        let _commit = basis::commit();
        let reply = Reply::new(self.request.id(), self.request.opcode(), outcome)
            .expect("operation result");
        self.actor.record_memory(accepted, reply);
        reply
    }
    pub(crate) fn finish(mut self, result: Result<memory::Value, Failure>) -> [u8; Reply::WIDTH] {
        let state = core::mem::replace(&mut self.state, State::Deferred);
        match state {
            State::Committed(reply) => reply,
            State::Deferred => self
                .actor
                .inspect_memory(self.request.opcode().domain(), self.request.id())
                .expect("retained result")
                .expect("accepted result"),
            State::Ready(accepted) => self.record(
                &accepted,
                match result {
                    Ok(value) => Outcome::Committed(value),
                    Err(failure) => Outcome::Rejected(failure),
                },
            ),
            State::Started { accepted, value } => self.record(
                &accepted,
                match result {
                    Ok(value) => Outcome::Committed(value),
                    Err(failure) => Outcome::Failed {
                        failure,
                        retained: Some(value),
                    },
                },
            ),
        }
        .to_bytes()
    }
}
