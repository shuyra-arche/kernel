use crate::basis::{self, unit::task::Task};
use alloc::sync::Arc;
use kernel_api::abi::memory::{self, Failure, Outcome, Reply, Request, journal::Accepted};

/// One accepted invocation. Resource owners report acceptance under COMMIT
/// before hardware effects. No journal or Task ledger lock crosses a fence.
pub(in crate::entry::call) struct Receipt<'a> {
    actor: &'a Arc<Task>,
    request: Request,
    accepted: Accepted,
    started: Option<memory::Value>,
}

impl<'a> Receipt<'a> {
    pub(super) fn new(actor: &'a Arc<Task>, request: Request, accepted: Accepted) -> Self {
        Self {
            actor,
            request,
            accepted,
            started: None,
        }
    }
    pub(in crate::entry::call) fn committed(&mut self, value: memory::Value) {
        self.record(Outcome::Committed(value));
    }

    pub(in crate::entry::call) fn start(&mut self, value: memory::Value) {
        self.started = Some(value);
        self.record(Outcome::Pending(value));
    }

    pub(in crate::entry::call) fn started(&self) -> Option<memory::Value> {
        self.started
    }

    fn record(&self, outcome: Outcome) -> Reply {
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
            .record(&self.accepted, reply)
            .expect("record accepted operation before output");
        reply
    }

    pub(super) fn finish(self, result: Result<memory::Value, Failure>) -> [u8; Reply::WIDTH] {
        self.record(match result {
            Ok(value) => Outcome::Committed(value),
            Err(failure) => match self.started {
                Some(value) => Outcome::Failed {
                    failure,
                    retained: Some(value),
                },
                None => Outcome::Rejected(failure),
            },
        })
        .to_bytes()
    }
}
