use alloc::sync::Arc;
use alloc::sync::Weak;
use alloc::vec::Vec;

use kernel_api::abi::call::pend::PendFail;
use kernel_api::abi::call::sign::SignEventFail;
use kernel_api::model::identity::TaskId;
use kernel_api::model::sign::SignSource;

use crate::basis::pend;
use crate::basis::sign::WakeKey;
use crate::basis::unit::life::Life;
use crate::basis::unit::task::Task;

use super::watch::Watch;

#[derive(Clone)]
pub(crate) struct Route {
    pub actor: TaskId,
    pub keys: Arc<Vec<(WakeKey, Weak<Life>)>>,
    pub watch: Watch,
}

impl Route {
    pub(super) fn watches(&self, actor: TaskId, source: SignSource) -> bool {
        self.actor == actor && self.watch.source() == source
    }

    pub(super) fn covers(&self, key: WakeKey) -> bool {
        self.keys.iter().any(|(known, _)| *known == key)
    }

    pub(super) fn poll(&self, actor: &Arc<Task>) -> Option<Option<SignEventFail>> {
        match &self.watch {
            Watch::Direct { token } => match actor
                .signs
                .get(*token, kernel_api::model::sign::SignRights::LISTEN)
            {
                Ok(endpoint) => endpoint.ready().then_some(None),
                Err(kernel_api::abi::call::sign::SignFail::Dead) => Some(Some(SignEventFail::Dead)),
                Err(_) => Some(Some(SignEventFail::Denied)),
            },
            Watch::ExternalMapping {
                token,
                mapping,
                fault,
            } => {
                match crate::basis::unit::space::external::event(actor, *token, *mapping, *fault) {
                    Ok((_, Some(true))) => Some(None),
                    Ok((_, Some(false))) => Some(Some(SignEventFail::Busy)),
                    Ok(_) => None,
                    Err(_) => Some(Some(SignEventFail::Denied)),
                }
            }
            Watch::Resource { source } => {
                let Some((token, need)) = super::codec::source_watch(*source) else {
                    return Some(Some(SignEventFail::Denied));
                };
                let snapshot = match pend::accede(actor, token, need) {
                    Ok(p) => p,
                    Err(e) => return Some(Some(super::codec::event_fail(e))),
                };
                if let Err(e) = pend::available(&snapshot) {
                    return Some(Some(super::codec::event_fail(e)));
                }
                let Some(ready) = super::codec::source_ready(&snapshot, *source) else {
                    return Some(Some(SignEventFail::Denied));
                };
                ready.then_some(None)
            }
            Watch::Join { target, selected } => {
                match crate::basis::unit::join::observe(actor, *target).and_then(|current| {
                    if selected.same(&current) {
                        Ok(current)
                    } else {
                        Err(kernel_api::abi::call::unit::UnitFail::Denied)
                    }
                }) {
                    Ok(crate::basis::unit::join::JoinTarget::Task(member)) => {
                        member.exit().is_some().then_some(None)
                    }
                    Ok(crate::basis::unit::join::JoinTarget::Team(team)) => {
                        (!team.admitting() && team.all_reaped()).then_some(None)
                    }
                    Err(_) => Some(Some(SignEventFail::Denied)),
                }
            }
            Watch::Inspect {
                token,
                holder,
                parent,
                held,
                access,
                ..
            } => {
                let Some(holder) = holder.upgrade() else {
                    return Some(Some(SignEventFail::Denied));
                };
                let Some(pend_entry) = pend::locate(&holder, *token) else {
                    return Some(Some(SignEventFail::Denied));
                };
                if !pend_entry.alive() {
                    return Some(Some(SignEventFail::Dead));
                }
                if !pend_entry.modifiable().allows(*access) {
                    return Some(Some(SignEventFail::Denied));
                }
                if let Some((lord, parent_token)) = parent {
                    let Some(lord) = lord.upgrade() else {
                        return Some(Some(SignEventFail::Denied));
                    };
                    if lord.ident.id != actor.ident.id
                        || pend::locate(&lord, *parent_token).is_none()
                        || pend_entry.sire() != Some(*parent_token)
                        || !pend_entry.lord().ptr_eq(&Arc::downgrade(&lord))
                    {
                        return Some(Some(SignEventFail::Denied));
                    }
                    match pend::available(&pend_entry) {
                        Ok(()) => None,
                        Err(e) => Some(Some(super::codec::event_fail(e))),
                    }
                } else {
                    match pend::available(&pend_entry) {
                        Ok(()) => (*held).then_some(None),
                        Err(PendFail::HandedOver) if *held => None,
                        Err(e) => Some(Some(super::codec::event_fail(e))),
                    }
                }
            }
        }
    }
}
