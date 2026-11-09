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
use crate::metal::switcher::context::TrapContext;
use alloc::sync::Arc;
use alloc::vec::Vec;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::abi::call::sign::SignCall;
use kernel_api::abi::call::sign::SignFail;
use kernel_api::model::identity::PendId;
use kernel_api::model::sign::SignRights;
use kernel_api::model::sign::SignSource;
use kernel_api::model::wait::Wait;

pub(super) fn dispatch(frame: &mut TrapContext, call: SignCall, ident: Arc<TaskIdent>) -> Outcome {
    drop(ident);
    match call {
        SignCall::Create { license } => answer(frame, create(license)),
        SignCall::Delete { token } => answer(
            frame,
            with_group(token, SignRights::DELETE, |_, group| {
                endpoint::seal(group);
                Ok(0)
            }),
        ),
        SignCall::Attach {
            token,
            source,
            generation,
        } => {
            answer(
                frame,
                attach(token, |task| typed_cell(task, token, source, generation)).map(|()| 0),
            );
        }
        SignCall::Detach { token, source } => {
            let result = with_group(token, SignRights::ROUTE, |_, group| {
                endpoint::detach(group, token, source)
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
            with_group(token, SignRights::CONSUME, |_, endpoint| {
                endpoint::consume(endpoint, sequence)?;
                Ok(0)
            }),
        ),
    }
    Outcome::Resume
}

pub(crate) fn create(license: PendId) -> Result<usize, SignFail> {
    let task = current().running_task().ok_or(SignFail::Denied)?;
    let _serial = task.pend.lock();
    let _commit = crate::basis::commit();
    if !pend::live(&task) {
        return Err(SignFail::Dead);
    }
    crate::basis::pend::access::license(
        &task,
        license,
        kernel_api::model::pend::CreationRights::SIGN,
    )
    .map_err(codec::pend_fail)?;
    let endpoint = endpoint::Endpoint::try_new(task.ident.id).map_err(|_| SignFail::OutOfMemory)?;
    let entry = pend::PendEntry::try_new(
        pend::Target::Sign(endpoint),
        kernel_api::model::identity::Mark::NONE,
        pend::Modifiable::No,
        None,
    )
    .map_err(codec::pend_fail)?;
    let token = entry.token();
    let entry = Arc::try_new(entry).map_err(|_| SignFail::OutOfMemory)?;
    task.pend
        .prepare(entry)
        .map_err(codec::pend_fail)?
        .publish();
    pend::changed(&task);
    Ok(token.get())
}

fn with_group<T>(
    token: PendId,
    need: SignRights,
    operation: impl FnOnce(&Arc<Task>, &Arc<endpoint::Endpoint>) -> Result<T, SignFail>,
) -> Result<T, SignFail> {
    let task = current().running_task().ok_or(SignFail::Denied)?;
    let _serial = task.pend.lock();
    let _commit = crate::basis::commit();
    if !pend::live(&task) {
        return Err(SignFail::Dead);
    }
    let endpoint = endpoint::authorized(&task, token, need)?;
    operation(&task, &endpoint)
}

fn attach(
    token: PendId,
    registration: impl FnOnce(&Arc<Task>) -> Result<Route, SignFail>,
) -> Result<(), SignFail> {
    with_group(token, SignRights::ROUTE, |task, group| {
        endpoint::attach(group, registration(task)?)
    })
}

fn await_group(frame: &mut TrapContext, token: PendId, millis: Wait) -> Outcome {
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

pub(crate) fn typed_cell(
    actor: &Arc<Task>,
    permit: PendId,
    source: SignSource,
    generation: u64,
) -> Result<Route, SignFail> {
    let memo = matches!(
        source,
        SignSource::MappingRetired { .. } | SignSource::MappingPublished { .. }
    );
    if memo == (generation == 0) {
        return Err(SignFail::Denied);
    }
    let mut keys = Vec::new();
    keys.try_reserve(4).map_err(|_| SignFail::OutOfMemory)?;
    let watch = match source {
        SignSource::MappingRetired { token, mapping }
        | SignSource::MappingPublished { token, mapping } => {
            let published = matches!(source, SignSource::MappingPublished { .. });
            let (life, _) = crate::basis::nest::authority::external::event(
                actor, token, mapping, published, generation,
            )
            .map_err(|_| SignFail::Denied)?;
            keys.push((WakeKey::ExternalMapping { id: mapping.get() }, life));
            keys.push((
                WakeKey::Inspect {
                    task: actor.ident.id,
                    token: token.get(),
                },
                actor.life(),
            ));
            Watch::ExternalMapping {
                token,
                mapping,
                published,
                generation,
            }
        }
        SignSource::NestFault { token } | SignSource::NestClosed { token } => {
            let fault = matches!(source, SignSource::NestFault { .. });
            let nest = crate::basis::nest::authority::binding::authorized(
                actor,
                token,
                kernel_api::model::pend::NestRights::INSPECT,
            )
            .map_err(|_| SignFail::Denied)?
            .target()
            .nest()
            .ok_or(SignFail::Denied)?;
            crate::basis::nest::authority::binding::event(actor, token, fault)
                .map_err(|_| SignFail::Denied)?;
            keys.push((WakeKey::ExternalNest { id: nest.id() }, nest.life()));
            keys.push((
                WakeKey::Inspect {
                    task: actor.ident.id,
                    token: token.get(),
                },
                actor.life(),
            ));
            Watch::Nest { token, fault }
        }
        SignSource::Sign { token } => {
            let source = endpoint::authorized(actor, token, SignRights::LISTEN)?;
            keys.push((endpoint::trigger_key(&source), source.life()));
            keys.push((
                WakeKey::Inspect {
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
            if let Some(wake) = pend.target().wake() {
                keys.push(wake);
            }
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
        permit,
        keys: Arc::try_new(keys).map_err(|_| SignFail::OutOfMemory)?,
        watch,
    })
}
