//! An interrupt producer owns the (Share View, Sign) association. Sign retains
//! ordinary events; only this owner interprets a successful consumption receipt.
mod state;
use crate::basis::{
    flat::sharing::Sharing,
    pend::{self, PendEntry},
    sign::endpoint::{self, Endpoint},
    unit::task::Task,
};
use crate::metal::{
    generic::interrupt::Irq,
    sync::{Level, OnceLock, SpinLock},
};
use alloc::sync::{Arc, Weak};
use kernel_api::{
    abi::call::{pend::PendFail, sign::SignFail},
    model::{
        identity::{PendId, TaskId},
        modifiable::Modifiable,
        pend::{Distributable, Operations, ShareRights},
        sign::SignRights,
    },
};
use state::{Holder, State};

pub(crate) struct Source {
    pub(crate) irq: Irq,
    pub(crate) region: usize,
    pub(crate) endpoint: Arc<Endpoint>,
    view: OnceLock<Arc<Sharing>>,
    binding: SpinLock<Binding>,
}
struct Binding {
    phase: State,
    task: Weak<Task>,
    view: Weak<PendEntry>,
    sign: Weak<PendEntry>,
    announce: bool,
}
impl Source {
    pub(super) fn new(irq: Irq, region: usize) -> Result<Self, PendFail> {
        Ok(Self {
            irq,
            region,
            endpoint: Endpoint::try_new(TaskId::new(0)).map_err(|_| PendFail::OutOfMemory)?,
            view: OnceLock::new(),
            binding: SpinLock::new_level(
                Level::L3,
                Binding {
                    phase: State::new(),
                    task: Weak::new(),
                    view: Weak::new(),
                    sign: Weak::new(),
                    announce: false,
                },
            ),
        })
    }
    pub(super) fn associate(&self, view: Arc<Sharing>) -> Result<(), PendFail> {
        self.view.set(view).map_err(|_| PendFail::Denied)
    }
    fn live(&self) -> bool {
        let (task, view, sign) = {
            let b = self.binding.lock();
            (b.task.upgrade(), b.view.upgrade(), b.sign.upgrade())
        };
        task.is_some_and(|t| pend::live(&t))
            && view.is_some_and(|v| {
                v.allows(pend::Need::Write) && v.permits(Operations::Share(ShareRights::OPEN))
            })
            && sign.is_some_and(|s| {
                s.permits(Operations::Sign(SignRights::LISTEN | SignRights::CONSUME))
            })
    }
    pub(super) fn bind(&self, task: &Arc<Task>, sign: &Arc<PendEntry>) -> Result<(), PendFail> {
        if !sign.permits(Operations::Sign(SignRights::CONSUME)) {
            return Ok(()); // observation-only derivations do not acquire the source
        }
        if sign.authority().limits().distributable != Distributable::No
            || !sign.permits(Operations::Sign(SignRights::LISTEN))
        {
            return Err(PendFail::Denied);
        }
        let shared = self.view.get().ok_or(PendFail::Denied)?;
        let view = task
            .pend
            .entries
            .lock()
            .iter()
            .find(|entry| {
                entry
                    .target()
                    .share_view()
                    .is_some_and(|v| Arc::ptr_eq(&v, shared))
                    && entry.modifiable() == Modifiable::Yes
                    && entry.allows(pend::Need::Write)
                    && entry.permits(Operations::Share(ShareRights::OPEN))
                    && entry.authority().window().is_none_or(|(offset, count)| {
                        offset.get() == 0 && count == shared.source().page_count()
                    })
            })
            .cloned()
            .ok_or(PendFail::Denied)?;
        if !self.live() {
            self.close();
        }
        let mut next = self.binding.lock().phase;
        // Preflight the exclusive binding before publishing any event.
        next.claim(Holder {
            actor: task.ident.id,
            permit: sign.token(),
        })
        .map_err(failure)?;
        let sequence = endpoint::publish(&self.endpoint).map_err(|_| PendFail::Busy)?;
        next.begin(sequence);
        *self.binding.lock() = Binding {
            phase: next,
            task: Arc::downgrade(task),
            view: Arc::downgrade(&view),
            sign: Arc::downgrade(sign),
            announce: true,
        };
        super::external().expect("source router").request();
        Ok(())
    }
    pub(super) fn authorize(&self, task: &Task, token: PendId) -> Result<(), SignFail> {
        if self.live()
            && self.binding.lock().phase.owned(Holder {
                actor: task.ident.id,
                permit: token,
            })
        {
            Ok(())
        } else {
            Err(SignFail::Denied)
        }
    }
    pub(super) fn withdraw(&self, token: PendId) {
        let closes = {
            let b = self.binding.lock();
            b.sign.upgrade().is_some_and(|s| s.token() == token)
                || b.view.upgrade().is_some_and(|v| v.token() == token)
        };
        if closes {
            self.close();
        }
    }
    fn close(&self) {
        if !self.binding.lock().phase.bound() {
            return;
        }
        if let Some(route) = super::external() {
            route.mask(self.irq);
        }
        {
            let mut b = self.binding.lock();
            b.phase.close();
            b.announce = true;
            b.task = Weak::new();
            b.view = Weak::new();
            b.sign = Weak::new();
        }
        endpoint::discard(&self.endpoint);
        super::external().expect("source router").request();
    }
    pub(super) fn deliver(&self) -> bool {
        if !self.live() {
            self.close();
        }
        if !self.binding.lock().phase.claimed() {
            return false;
        }
        match endpoint::publish(&self.endpoint) {
            Ok(sequence) => {
                let mut b = self.binding.lock();
                if b.phase.deliver(sequence).is_ok() {
                    b.announce = true;
                    return true;
                }
            }
            Err(_) => self.close(),
        }
        false
    }
    pub(super) fn maintain(&self) {
        if !self.live() {
            self.close();
        }
        let consumed = self.endpoint.consumed();
        let (complete, announce) = {
            let mut b = self.binding.lock();
            let complete = b.phase.consume(consumed);
            let announce = core::mem::take(&mut b.announce);
            (complete, announce)
        };
        if let Some(held) = complete {
            let route = super::external().expect("source router");
            if held {
                route.finish(self.irq, true);
            } else {
                route.unmask(self.irq);
            }
        }
        if announce && endpoint::announce(&self.endpoint) {
            self.binding.lock().announce = true;
            super::external().expect("source router").request();
        }
    }
}
fn failure(error: state::Error) -> PendFail {
    match error {
        state::Error::Busy => PendFail::Busy,
        state::Error::Stale => PendFail::Denied,
        state::Error::Exhausted => PendFail::OutOfMemory,
    }
}

// SAFETY: PendId's API marker prevents user-side ownership duplication. Here IDs
// only identify ledger records; every binding transition is serialized by COMMIT
// and this lock, and records themselves are retained through synchronized Weak.
unsafe impl Send for Binding {}
