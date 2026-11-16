//! Exact live handler branches and their existing Sign notification edges.
use super::*;
use crate::basis::{
    pend::PendEntry,
    sign::endpoint::{self, Endpoint},
};
use kernel_api::model::pend::Operations;
use kernel_api::model::{
    identity::{PendId, TaskId},
    sign::SignRights,
};

pub(super) struct Binding {
    actor: Weak<Task>,
    pub handler: TaskId,
    reservation: Arc<pend::Pend>,
    token: PendId,
    sign: Arc<pend::Pend>,
    reservation_key: sign::WakeKey,
    pub endpoint: Arc<Endpoint>,
    wake: Arc<Endpoint>,
    keys: [sign::WakeKey; 4],
    pub config: FaultRoute,
}
// PendId's userspace !Send marker does not transfer a grant here. These are
// immutable lookup keys paired with the original Task and exact Pend lineage;
// all admission and replacement is serialized by COMMIT.
unsafe impl Send for Binding {}
unsafe impl Sync for Binding {}
impl Binding {
    pub fn prepare(
        actor: &Arc<Task>,
        reservation: &Arc<PendEntry>,
        span: &Arc<Span>,
        config: FaultRoute,
    ) -> Result<Arc<Self>, Failure> {
        if !config.valid() {
            return Err(Failure::Invalid);
        }
        let rights = SignRights::LISTEN | SignRights::ROUTE | SignRights::TRIGGER;
        let sign = pend::locate(actor, config.sign).ok_or(Failure::Denied)?;
        if !sign.permits(Operations::Sign(rights)) {
            return Err(Failure::Denied);
        }
        let endpoint = sign
            .target()
            .sign()
            .filter(|sign| sign.alive())
            .ok_or(Failure::Denied)?;
        let wake = Endpoint::try_new(actor.ident.id).map_err(|_| Failure::OutOfMemory)?;
        let nest = span.owner().ok_or(Failure::Dead)?;
        let keys = [
            sign::WakeKey::Task { id: actor.ident.id },
            sign::WakeKey::Inspect {
                task: actor.ident.id,
                token: reservation.token().get(),
            },
            sign::WakeKey::Inspect {
                task: actor.ident.id,
                token: config.sign.get(),
            },
            endpoint::trigger_key(&endpoint),
        ];
        // The reservation key is also watched, but separately: it has Nest life.
        let binding = Arc::try_new(Self {
            actor: Arc::downgrade(actor),
            handler: actor.ident.id,
            reservation: reservation.authority().clone(),
            token: reservation.token(),
            sign: sign.authority().clone(),
            reservation_key: span_key(&nest, span),
            endpoint,
            wake,
            keys,
            config,
        })
        .map_err(|_| Failure::OutOfMemory)?;
        for key in keys {
            let life = if key == endpoint::trigger_key(&binding.endpoint) {
                binding.endpoint.life()
            } else {
                actor.life()
            };
            sign::forward(key, life, binding.wake.id, binding.wake.life())
                .map_err(|_| Failure::OutOfMemory)?;
        }
        sign::forward(
            span_key(&nest, span),
            nest.life(),
            binding.wake.id,
            binding.wake.life(),
        )
        .map_err(|_| Failure::OutOfMemory)?;
        Ok(binding)
    }
    pub fn key(&self) -> sign::WakeKey {
        sign::WakeKey::Sign { id: self.wake.id }
    }
    pub fn life(&self) -> Weak<crate::basis::unit::anchor::Anchor> {
        self.wake.life()
    }
    pub fn wake_later(&self) {
        endpoint::delivery::enqueue(&self.wake);
    }
    pub fn live(&self, span: &Span) -> bool {
        let Some(actor) = self.actor.upgrade().filter(|actor| pend::live(actor)) else {
            return false;
        };
        let Some(reservation) = pend::locate(&actor, self.token)
            .filter(|entry| Arc::ptr_eq(entry.authority(), &self.reservation))
        else {
            return false;
        };
        let Some(sign) = pend::locate(&actor, self.config.sign)
            .filter(|entry| Arc::ptr_eq(entry.authority(), &self.sign))
        else {
            return false;
        };
        exact(&actor, &reservation)
            && reservation.permits(Operations::Nest(kernel_api::abi::memory::rights::FAULT))
            && full(&reservation, span)
            && exact(&actor, &sign)
            && sign.permits(Operations::Sign(
                SignRights::LISTEN | SignRights::ROUTE | SignRights::TRIGGER,
            ))
            && self.endpoint.alive()
            && span.is_open()
            && span.owner().is_some_and(|nest| nest.is_open())
    }
    pub fn reads(&self, actor: &Arc<Task>, token: PendId) -> bool {
        pend::locate(actor, self.token).is_some_and(|reservation| {
            Arc::ptr_eq(reservation.authority(), &self.reservation)
                && self.handler == actor.ident.id
                && self.token == token
                && exact(actor, &reservation)
                && reservation.permits(Operations::Nest(
                    kernel_api::model::pend::NestRights::INSPECT
                        | kernel_api::abi::memory::rights::FAULT,
                ))
        })
    }
    pub fn notify(&self) -> Result<(), Failure> {
        if self.endpoint.sequence().is_none() {
            endpoint::publish(&self.endpoint).map_err(|_| Failure::HandlerGone)?;
        }
        endpoint::delivery::enqueue(&self.endpoint);
        Ok(())
    }
}
fn exact(actor: &Arc<Task>, entry: &Arc<PendEntry>) -> bool {
    pend::locate(actor, entry.token()).is_some_and(|current| Arc::ptr_eq(&current, entry))
}
pub(super) fn full(entry: &PendEntry, span: &Span) -> bool {
    entry.authority().window().is_some_and(|(offset, count)| {
        offset.get() == 0 && count.checked_bytes() == Some(span.initial_size())
    })
}
impl Drop for Binding {
    fn drop(&mut self) {
        for key in self.keys {
            sign::unforward(key, self.wake.id);
        }
        sign::unforward(self.reservation_key, self.wake.id);
    }
}
