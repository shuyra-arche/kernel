//! Result delivery only. The Memo retains all physical cleanup authority.
use super::Task;
#[path = "memory_result/transition.rs"]
mod transition;
use alloc::sync::{Arc, Weak};
use kernel_api::abi::memory::{Outcome, Reply, Request, Value, journal::Accepted};
pub(crate) use transition::Transition;

pub(crate) struct Completion {
    actor: Weak<Task>,
    request: Request,
    accepted: Accepted,
    value: Value,
}
// SAFETY: request PendIds are immutable result metadata, never resolved as
// authority here. Task journal access is serialized; only the owning Memo may
// consume this callback, including when its cleanup migrates to another Hart.
unsafe impl Send for Completion {}
impl Completion {
    pub(crate) fn new(
        actor: &Arc<Task>,
        request: Request,
        accepted: Accepted,
        value: Value,
    ) -> Self {
        Self {
            actor: Arc::downgrade(actor),
            request,
            accepted,
            value,
        }
    }
    /// Called only after the owning Memo's exact hardware transition, outside
    /// the Nest lock. No grant is minted or resource retired by this callback.
    pub(crate) fn finish(self, transition: Transition) {
        let outcome = transition.outcome(self.request.action(), self.value);
        self.record(outcome);
    }
    fn record(self, outcome: Outcome) {
        let Some(actor) = self.actor.upgrade() else {
            return;
        };
        let reply = Reply::new(
            self.request.id().unwrap(),
            self.request.action().opcode(),
            outcome,
        )
        .unwrap();
        if let Some(journal) = actor.memory_results.lock().as_mut() {
            journal
                .record(&self.accepted, reply)
                .expect("exact pending Memo result");
        }
    }
}
