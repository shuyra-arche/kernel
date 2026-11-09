mod state;
use crate::basis::sign::endpoint::{self, Endpoint};
use crate::metal::{
    generic::interrupt::Irq,
    sync::{Level, SpinLock},
};
use kernel_api::{
    abi::call::sign::SignFail,
    model::identity::{PendId, TaskId},
};
use state::{Holder, State};

pub(crate) struct Source {
    pub(crate) irq: Irq,
    state: SpinLock<State>,
}
impl Source {
    pub(super) fn new(irq: Irq) -> Self {
        Self {
            irq,
            state: SpinLock::new_level(Level::L3, State::new()),
        }
    }
    pub(crate) fn owned(&self, actor: TaskId, permit: PendId) -> bool {
        self.state.lock().owned(Holder { actor, permit })
    }
    pub(crate) fn claim(&self, actor: TaskId, permit: PendId) -> Result<u64, SignFail> {
        self.state
            .lock()
            .claim(Holder { actor, permit })
            .map_err(failure)
    }
    pub(crate) fn rearm(
        &self,
        endpoint: &Endpoint,
        actor: TaskId,
        permit: PendId,
        generation: u64,
        sequence: u64,
    ) -> Result<(), SignFail> {
        let route = super::external().ok_or(SignFail::Dead)?;
        let mut next = *self.state.lock();
        let complete = next
            .rearm(Holder { actor, permit }, generation, sequence)
            .map_err(failure)?;
        if sequence != 0 {
            if endpoint.sequence() != Some(sequence) {
                return Err(SignFail::Denied);
            }
            endpoint::consume_hardware(endpoint, sequence)?;
        }
        *self.state.lock() = next;
        if complete {
            route.finish(self.irq, true);
        } else {
            route.unmask(self.irq);
        }
        Ok(())
    }
    pub(crate) fn yield_binding(
        &self,
        endpoint: &Endpoint,
        actor: TaskId,
        permit: PendId,
        generation: u64,
    ) -> Result<(), SignFail> {
        self.state
            .lock()
            .yield_binding(Holder { actor, permit }, generation)
            .map_err(failure)?;
        self.close(endpoint);
        Ok(())
    }
    pub(crate) fn revoke(&self, endpoint: &Endpoint, permit: PendId) {
        if self.state.lock().revoke(permit) {
            self.close(endpoint);
        }
    }
    pub(crate) fn seal(&self, endpoint: &Endpoint) {
        self.state.lock().close();
        self.close(endpoint);
    }
    fn close(&self, endpoint: &Endpoint) {
        if let Some(route) = super::external() {
            route.mask(self.irq);
        }
        if let Some(sequence) = endpoint.sequence() {
            let _ = endpoint::consume_hardware(endpoint, sequence);
        }
        endpoint::authority_changed(endpoint);
    }
    pub(super) fn deliver(endpoint: &Endpoint) {
        let Some(source) = endpoint.interrupt() else {
            return;
        };
        if !source.state.lock().claimed() {
            return;
        }
        if endpoint::trigger_hardware(endpoint).is_err() {
            endpoint::seal(endpoint);
            return;
        }
        if let Some(sequence) = endpoint.sequence() {
            let _ = source.state.lock().deliver(sequence);
        }
    }
}
fn failure(error: state::Error) -> SignFail {
    match error {
        state::Error::Busy => SignFail::Busy,
        state::Error::Stale => SignFail::Denied,
        state::Error::Exhausted => SignFail::OutOfMemory,
    }
}
