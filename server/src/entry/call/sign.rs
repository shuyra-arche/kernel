use super::outcome::{Outcome, answer, answer_pair};
use crate::core::work::pend;
use crate::core::work::room::Handoff;
use crate::core::work::room::conductor::current;
use crate::core::work::room::scheduler::roster::muster;
use crate::core::work::sign::WakeKey;
use crate::core::work::sign::endpoint::{self, AwaitWait, Route, Watch, codec};
use crate::core::work::unit::task::{Task, TaskIdent};
use crate::machine::switcher::context::TrapContext;
use alloc::sync::Arc;
use alloc::vec::Vec;
use kernel_api::{
    abi::call::pend::PendFail,
    abi::call::sign::SignCall,
    abi::call::sign::SignFail,
    model::sign::{SignId, SignRights, SignSource},
    model::wait::Wait,
};

pub(super) fn dispatch(frame: &mut TrapContext, call: SignCall, ident: Arc<TaskIdent>) -> Outcome {
    drop(ident);
    match call {
        SignCall::Next { after } => answer_pair(
            frame,
            current()
                .running_task()
                .ok_or(SignFail::Denied)
                .map(|task| task.signs.next(after)),
        ),
        SignCall::Create => answer(frame, create()),
        SignCall::Close { token } => answer(
            frame,
            (|| {
                let _commit = crate::core::work::commit();
                let task = current().running_task().ok_or(SignFail::Denied)?;
                task.signs.close(task.ident.id, token).map(|()| 0)
            })(),
        ),
        SignCall::Release { token } => answer(
            frame,
            (|| {
                let _commit = crate::core::work::commit();
                let task = current().running_task().ok_or(SignFail::Denied)?;
                task.signs.release(task.ident.id, token).map(|()| 0)
            })(),
        ),
        SignCall::Delegate {
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
                crate::core::work::sign::delegate(&task, &target, token, rights).map(SignId::get)
            })(),
        ),
        SignCall::Route { token, source } => {
            answer(
                frame,
                attach(token, |task| typed_cell(task, source)).map(|()| 0),
            );
        }
        SignCall::Unroute { token, source } => {
            let _commit = crate::core::work::commit();
            let result = with_group(token, SignRights::ROUTE, |task, group| {
                endpoint::detach(group, task.ident.id, source)
            });
            answer(frame, result.map(|()| 0));
        }
        SignCall::Listen { token, millis } => return await_group(frame, token, millis),
        SignCall::Trigger { token } => answer(
            frame,
            with_group(token, SignRights::TRIGGER, |_, endpoint| {
                endpoint::trigger(endpoint)
            })
            .map(|()| 0),
        ),
        SignCall::Consume { token } => answer(
            frame,
            with_group(token, SignRights::LISTEN, |_, endpoint| {
                endpoint::consume(endpoint)?;
                crate::core::work::room::interrupt::acknowledge(endpoint);
                Ok(0)
            }),
        ),
    }
    Outcome::Resume
}

pub(crate) fn create() -> Result<usize, SignFail> {
    let _commit = crate::core::work::commit();
    let task = current().running_task().ok_or(SignFail::Denied)?;
    if !pend::live(&task) {
        return Err(SignFail::Dead);
    }
    let endpoint = endpoint::Endpoint::try_new(task.ident.id).map_err(|_| SignFail::OoM)?;
    task.signs
        .install(endpoint, SignRights::all(), true)
        .map(SignId::get)
}

fn with_group<T>(
    token: SignId,
    need: SignRights,
    operation: impl FnOnce(&Arc<Task>, &Arc<endpoint::Endpoint>) -> Result<T, SignFail>,
) -> Result<T, SignFail> {
    let _commit = crate::core::work::commit();
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
    let _commit = crate::core::work::commit();
    with_group(token, SignRights::ROUTE, |task, group| {
        endpoint::attach(group, registration(task)?)
    })
}

fn await_group(frame: &mut TrapContext, token: SignId, millis: Wait) -> Outcome {
    let result = with_group(token, SignRights::LISTEN, |task, group| {
        Ok(AwaitWait::new(group.clone(), token, task, millis))
    })
    .and_then(crate::core::work::sign::endpoint::wait::wait);
    match result {
        Ok(Handoff::Resume(reply)) => AwaitWait::write_result(frame, Ok(reply)),
        Ok(Handoff::Switch(pa)) => return Outcome::Switch(pa as *mut TrapContext),
        Err(error) => AwaitWait::write_result(frame, Err(error)),
    }
    Outcome::Resume
}

pub(crate) fn typed_cell(actor: &Arc<Task>, source: SignSource) -> Result<Route, SignFail> {
    let mut keys = Vec::new();
    keys.try_reserve(4).map_err(|_| SignFail::OoM)?;
    let watch = match source {
        SignSource::DeviceRetired { device } | SignSource::DeviceFault { device } => {
            let fault = matches!(source, SignSource::DeviceFault { .. });
            let (life, _) = crate::machine::device::event(actor, device, fault)
                .map_err(|_| SignFail::Denied)?;
            keys.push((WakeKey::Device { id: device }, life));
            Watch::Device { id: device, fault }
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
            let selected = crate::core::work::unit::join::observe(actor, target)
                .map_err(|_| SignFail::Denied)?;
            let wait = crate::core::work::unit::join::JoinWait::new(
                selected.clone(),
                actor.ident.id,
                false,
                Wait::POLL,
            );
            keys.push((wait.target.key(), wait.target.life()));
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
        keys: Arc::try_new(keys).map_err(|_| SignFail::OoM)?,
        watch,
    })
}
