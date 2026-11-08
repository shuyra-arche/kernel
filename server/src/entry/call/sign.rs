use super::outcome::Outcome;
use super::outcome::answer;
use crate::basis::pend;
use crate::basis::room::Handoff;
use crate::basis::room::conductor::current;
use crate::basis::room::scheduler::roster::muster;
use crate::basis::sign::WakeKey;
use crate::basis::sign::endpoint;
use crate::basis::sign::endpoint::AwaitWait;
use crate::basis::sign::endpoint::Route;
use crate::basis::sign::endpoint::Watch;
use crate::basis::sign::endpoint::codec;
use crate::basis::unit::task::Task;
use crate::basis::unit::task::TaskIdent;
use crate::machine::switcher::context::TrapContext;
use alloc::sync::Arc;
use alloc::vec::Vec;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::abi::call::sign::SignCall;
use kernel_api::abi::call::sign::SignFail;
use kernel_api::model::sign::SignId;
use kernel_api::model::sign::SignRights;
use kernel_api::model::sign::SignSource;
use kernel_api::model::wait::Wait;

pub(super) fn dispatch(frame: &mut TrapContext, call: SignCall, ident: Arc<TaskIdent>) -> Outcome {
    drop(ident);
    match call {
        SignCall::Revoke {
            token,
            target,
            branch,
        } => answer(
            frame,
            (|| {
                let _commit = crate::basis::commit();
                let task = current().running_task().ok_or(SignFail::Denied)?;
                let target = muster(target)
                    .and_then(|weak| weak.upgrade())
                    .ok_or(SignFail::Denied)?;
                task.signs.revoke(token, &target.signs, branch).map(|()| 0)
            })(),
        ),
        SignCall::Create => answer(frame, create()),
        SignCall::Delete { token } => answer(
            frame,
            (|| {
                let _commit = crate::basis::commit();
                let task = current().running_task().ok_or(SignFail::Denied)?;
                task.signs.close(task.ident.id, token).map(|()| 0)
            })(),
        ),
        SignCall::Return { token } => answer(
            frame,
            (|| {
                let _commit = crate::basis::commit();
                let task = current().running_task().ok_or(SignFail::Denied)?;
                task.signs.release(task.ident.id, token).map(|()| 0)
            })(),
        ),
        SignCall::Accord {
            token,
            target,
            rights,
        } => answer(
            frame,
            (|| {
                let task = current().running_task().ok_or(SignFail::Denied)?;
                let target = muster(target)
                    .and_then(|weak| weak.upgrade())
                    .ok_or(SignFail::Denied)?;
                crate::basis::sign::delegate(&task, &target, token, rights).map(SignId::get)
            })(),
        ),
        SignCall::Attach { token, source } => {
            answer(
                frame,
                attach(token, |task| typed_cell(task, source)).map(|()| 0),
            );
        }
        SignCall::Detach { token, source } => {
            let _commit = crate::basis::commit();
            let result = with_group(token, SignRights::ROUTE, |task, group| {
                endpoint::detach(group, task.ident.id, source)
            });
            answer(frame, result.map(|()| 0));
        }
        SignCall::Listen { token, wait } => return await_group(frame, token, wait),
        SignCall::Trigger { token } => answer(
            frame,
            with_group(token, SignRights::TRIGGER, |_, endpoint| {
                endpoint::trigger(endpoint)
            })
            .map(|()| 0),
        ),
        SignCall::Consume { token, sequence } => answer(
            frame,
            with_group(token, SignRights::LISTEN, |_, endpoint| {
                endpoint::consume(endpoint, sequence)?;
                Ok(0)
            }),
        ),
    }
    Outcome::Resume
}

pub(crate) fn create() -> Result<usize, SignFail> {
    let _commit = crate::basis::commit();
    let task = current().running_task().ok_or(SignFail::Denied)?;
    if !pend::live(&task) {
        return Err(SignFail::Dead);
    }
    let endpoint = endpoint::Endpoint::try_new(task.ident.id).map_err(|_| SignFail::OutOfMemory)?;
    task.signs
        .install(endpoint, SignRights::all(), true)
        .map(SignId::get)
}

fn with_group<T>(
    token: SignId,
    need: SignRights,
    operation: impl FnOnce(&Arc<Task>, &Arc<endpoint::Endpoint>) -> Result<T, SignFail>,
) -> Result<T, SignFail> {
    let _commit = crate::basis::commit();
    let task = current().running_task().ok_or(SignFail::Denied)?;
    if !pend::live(&task) {
        return Err(SignFail::Dead);
    }
    let endpoint = task.signs.get(token, need)?;
    operation(&task, &endpoint)
}

fn attach(
    token: SignId,
    registration: impl FnOnce(&Arc<Task>) -> Result<Route, SignFail>,
) -> Result<(), SignFail> {
    let _commit = crate::basis::commit();
    with_group(token, SignRights::ROUTE, |task, group| {
        endpoint::attach(group, registration(task)?)
    })
}

fn await_group(frame: &mut TrapContext, token: SignId, millis: Wait) -> Outcome {
    let result = with_group(token, SignRights::LISTEN, |task, group| {
        Ok(AwaitWait::new(group.clone(), token, task, millis))
    })
    .and_then(crate::basis::sign::endpoint::wait::wait);
    match result {
        Ok(Handoff::Resume(reply)) => AwaitWait::write_result(frame, Ok(reply)),
        Ok(Handoff::Switch(pa)) => return Outcome::Switch(pa as *mut TrapContext),
        Err(error) => AwaitWait::write_result(frame, Err(error)),
    }
    Outcome::Resume
}

pub(crate) fn typed_cell(actor: &Arc<Task>, source: SignSource) -> Result<Route, SignFail> {
    let mut keys = Vec::new();
    keys.try_reserve(4).map_err(|_| SignFail::OutOfMemory)?;
    let watch = match source {
        SignSource::MappingRetired { token, mapping }
        | SignSource::MappingFault { token, mapping } => {
            let fault = matches!(source, SignSource::MappingFault { .. });
            let (life, _) = crate::basis::cell::projection::event(actor, token, mapping, fault)
                .map_err(|_| SignFail::Denied)?;
            keys.push((WakeKey::ExternalMapping { id: mapping.get() }, life));
            Watch::ExternalMapping {
                token,
                mapping,
                fault,
            }
        }
        SignSource::Sign { token } => {
            let source = actor.signs.get(token, SignRights::LISTEN)?;
            keys.push((endpoint::trigger_key(&source), source.life()));
            keys.push((
                WakeKey::SignHandle {
                    task: actor.ident.id,
                    token: token.get(),
                },
                actor.life(),
            ));
            Watch::Direct { token }
        }
        SignSource::Clone { .. } => {
            let (token, need) = codec::source_watch(source).ok_or(SignFail::Denied)?;
            let pend = pend::accede(actor, token, need).map_err(codec::pend_fail)?;
            pend::available(&pend).map_err(codec::pend_fail)?;
            let (key, life) = codec::source_key(&pend, source).ok_or(SignFail::Denied)?;
            keys.push((key, life));
            keys.push((
                WakeKey::Inspect {
                    task: actor.ident.id,
                    token: token.get(),
                },
                actor.life(),
            ));
            Watch::Resource { source }
        }
        SignSource::Join { target } => {
            let selected = crate::basis::unit::lifecycle::join::observe(actor, target)
                .map_err(|_| SignFail::Denied)?;
            keys.push((selected.key(), selected.life()));
            Watch::Join { target, selected }
        }
        SignSource::Inspect { task: id, token } => {
            let holder = muster(id)
                .and_then(|task| task.upgrade())
                .ok_or(SignFail::Denied)?;
            let pend = pend::locate(&holder, token).ok_or(SignFail::Denied)?;
            if !pend.alive() {
                return Err(SignFail::Dead);
            }
            let parent = if id == actor.ident.id {
                None
            } else {
                let parent = pend.sire().ok_or(SignFail::Denied)?;
                if !pend.lord().ptr_eq(&Arc::downgrade(actor))
                    || pend::locate(actor, parent).is_none()
                {
                    return Err(SignFail::Denied);
                }
                keys.push((
                    WakeKey::Inspect {
                        task: actor.ident.id,
                        token: parent.get(),
                    },
                    actor.life(),
                ));
                Some((Arc::downgrade(actor), parent))
            };
            let held =
                parent.is_none() && matches!(pend::available(&pend), Err(PendFail::HandedOver));
            let view = pend.view();
            keys.push((WakeKey::seal(view.kind(), view.id()), view.life()));
            keys.push((
                WakeKey::Inspect {
                    task: id,
                    token: token.get(),
                },
                holder.life(),
            ));
            Watch::Inspect {
                task: id,
                token,
                holder: Arc::downgrade(&holder),
                parent,
                held,
                access: pend.modifiable(),
            }
        }
    };
    Ok(Route {
        actor: actor.ident.id,
        keys: Arc::try_new(keys).map_err(|_| SignFail::OutOfMemory)?,
        watch,
    })
}
