//! Result delivery only. The Memo retains all physical cleanup authority.
use super::Task;
use crate::{
    basis,
    basis::sync::{LegacyLock, order},
};
use alloc::boxed::Box;
mod transition;
use alloc::sync::{Arc, Weak};
use kernel_api::abi::memory::{
    self, Domain, Failure, Outcome, Reply, RequestId, Value,
    journal::{Accepted, Journal, RequestKey},
};
mod receipt;
pub(crate) use receipt::Receipt;

/// One bounded result owner per actual Task. Resource and hardware state stay
/// with Flat/Nest/Memo; this window only owns admission and response history.
pub(super) struct Requests(LegacyLock<order::TaskJournal, Option<Box<Journal>>>);
impl Requests {
    pub(super) const fn new() -> Self {
        Self(LegacyLock::new(None))
    }
}

pub(crate) enum Admission<'a> {
    Replay(Reply),
    Execute(Receipt<'a>),
}

impl Task {
    /// Reserve result capacity before returning the sole execution receipt.
    /// Replays never return a permit to run effects again. All admission guards
    /// end here, before Entry prepares a Page or starts a hardware operation.
    pub(crate) fn admit_memory_key(
        self: &Arc<Self>,
        request: RequestKey,
    ) -> Result<Admission<'_>, Failure> {
        let _serial = self.pend.lock();
        let _commit = basis::commit();
        let mut window = self.requests.0.lock();
        if window.is_none() {
            // Initialize Journal directly in fallible heap storage. Creating
            // Journal::new() by value uses too much guarded trap-stack space.
            let actor = self.ident.id;
            if actor.get() == 0 || actor.get() > isize::MAX as usize {
                return Err(Failure::Denied);
            }
            let mut storage = Box::<Journal>::try_new_uninit().map_err(|_| Failure::OutOfMemory)?;
            let target = storage.as_mut().as_mut_ptr();
            // SAFETY: target is the unique aligned Box allocation. Write all
            // fields and each record before publishing the initialized Box.
            // No uninitialized reference to Journal is ever formed.
            unsafe {
                core::ptr::addr_of_mut!((*target).actor).write(actor);
                core::ptr::addr_of_mut!((*target).high_water).write(0);
                let records = core::ptr::addr_of_mut!((*target).records)
                    .cast::<Option<kernel_api::abi::memory::journal::Record>>();
                for slot in 0..kernel_api::abi::memory::RESULT_CAPACITY {
                    records.add(slot).write(None);
                }
                *window = Some(storage.assume_init());
            }
        }
        let journal = window.as_mut().expect("prepared Task journal");
        match journal.begin(self.ident.id, request)? {
            memory::journal::Admission::Replay(reply)
            | memory::journal::Admission::InFlight(Some(reply)) => Ok(Admission::Replay(reply)),
            memory::journal::Admission::InFlight(None) => Err(Failure::Busy),
            memory::journal::Admission::Execute(accepted) => {
                Ok(Admission::Execute(Receipt::new(self, request, accepted)))
            }
        }
    }

    pub(crate) fn inspect_result(
        &self,
        domain: Domain,
        id: RequestId,
    ) -> Result<Option<Reply>, Failure> {
        self.inspect_memory(domain, id)
    }

    fn inspect_memory(&self, domain: Domain, id: RequestId) -> Result<Option<Reply>, Failure> {
        self.requests
            .0
            .lock()
            .as_ref()
            .ok_or(Failure::Busy)?
            .inspect(self.ident.id, domain, id)
    }

    fn record_memory(&self, accepted: &Accepted, reply: Reply) {
        let mut window = self.requests.0.lock();
        if let Some(journal) = window.as_mut() {
            journal
                .record(accepted, reply)
                .expect("exact accepted Task result");
        }
    }
}

pub(crate) use transition::Transition;

pub(crate) struct Completion {
    actor: Weak<Task>,
    request: RequestKey,
    accepted: Accepted,
    value: Value,
}
// SAFETY: request PendIds are immutable result metadata, never resolved as
// authority here. Task journal access is serialized; only the owning Memo may
// consume this callback, including when its cleanup migrates to another Hart.
unsafe impl Send for Completion {}
impl Completion {
    fn new(actor: &Arc<Task>, request: RequestKey, accepted: Accepted, value: Value) -> Self {
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
        let outcome = transition.outcome(self.request.opcode(), self.value);
        self.record(outcome);
    }
    fn record(self, outcome: Outcome) {
        let Some(actor) = self.actor.upgrade() else {
            return;
        };
        let reply = Reply::new(self.request.id(), self.request.opcode(), outcome).unwrap();
        actor.record_memory(&self.accepted, reply);
    }
}
