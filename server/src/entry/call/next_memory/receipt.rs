use crate::basis::{self, unit::task::Task};
use alloc::sync::Arc;
use kernel_api::abi::memory::{self, Failure, Outcome, Reply, Request, journal::Accepted};

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

pub(in crate::entry::call) struct Receipt<'a> {
    actor: &'a Arc<Task>,
    request: Request,
    state: State,
}
impl<'a> Receipt<'a> {
    pub(super) fn new(actor: &'a Arc<Task>, request: Request, accepted: Accepted) -> Self {
        Self {
            actor,
            request,
            state: State::Ready(accepted),
        }
    }
    pub(in crate::entry::call) fn committed(&mut self, value: memory::Value) {
        let accepted = match core::mem::replace(&mut self.state, State::Deferred) {
            State::Ready(accepted) | State::Started { accepted, .. } => accepted,
            _ => panic!("completion already transferred"),
        };
        self.state = State::Committed(self.record(&accepted, Outcome::Committed(value)));
    }
    pub(in crate::entry::call) fn start(&mut self, value: memory::Value) {
        let accepted = match core::mem::replace(&mut self.state, State::Deferred) {
            State::Ready(accepted) | State::Started { accepted, .. } => accepted,
            _ => panic!("completion already transferred"),
        };
        self.record(&accepted, Outcome::Pending(value));
        self.state = State::Started { accepted, value };
    }
    pub(in crate::entry::call) fn started(&self) -> Option<memory::Value> {
        match self.state {
            State::Started { value, .. } => Some(value),
            _ => None,
        }
    }
    pub(in crate::entry::call) fn defer(
        &mut self,
    ) -> crate::basis::unit::task::memory_result::Completion {
        let State::Started { accepted, value } =
            core::mem::replace(&mut self.state, State::Deferred)
        else {
            panic!("only accepted work can transfer completion")
        };
        crate::basis::unit::task::memory_result::Completion::new(
            self.actor,
            self.request,
            accepted,
            value,
        )
    }
    fn record(&self, accepted: &Accepted, outcome: Outcome) -> Reply {
        let _commit = basis::commit();
        let reply = Reply::new(
            self.request.id().expect("mutation identity"),
            self.request.action().opcode(),
            outcome,
        )
        .expect("operation result");
        self.actor
            .memory_results
            .lock()
            .as_mut()
            .expect("admitted Task journal")
            .record(accepted, reply)
            .expect("record accepted operation before output");
        reply
    }
    pub(super) fn finish(mut self, result: Result<memory::Value, Failure>) -> [u8; Reply::WIDTH] {
        let state = core::mem::replace(&mut self.state, State::Deferred);
        match state {
            State::Committed(reply) => reply,
            State::Deferred => self
                .actor
                .memory_results
                .lock()
                .as_ref()
                .unwrap()
                .inspect(
                    self.actor.ident.id,
                    self.request.action().opcode().domain(),
                    self.request.id().unwrap(),
                )
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
