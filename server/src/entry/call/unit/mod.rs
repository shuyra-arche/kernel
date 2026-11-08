use alloc::sync::Arc;
use alloc::vec::Vec;

use kernel_api::abi::call::unit::UnitCall;
use kernel_api::abi::call::unit::UnitFail;
use kernel_api::model::unit::UnitTarget;

use crate::basis::nest::NestError;
use crate::basis::nest::SpaceKind;
use crate::basis::room::scheduler::roster::muster;
use crate::basis::unit::space::Space;
use crate::basis::unit::task::MAX_ARGS;
use crate::basis::unit::task::Origin;
use crate::basis::unit::task::Task;
use crate::basis::unit::task::TaskIdent;
use crate::basis::unit::task::TaskWeak;
use crate::machine::switcher::context::Gprs;
use crate::machine::switcher::context::TrapContext;
use kernel_api::address::VirtAddr as KVirt;

use super::outcome::Outcome;

fn map_err(e: NestError) -> UnitFail {
    match e {
        NestError::OutOfMemory => UnitFail::OoM,
        NestError::Busy => UnitFail::Busy,
        // 承载已移出：Unit 域没有对应的失败词，也不新造一个看起来等价的码。
        NestError::Missing => UnitFail::Denied,
        NestError::NotAligned
        | NestError::AlreadyMapped
        | NestError::NotMapped
        | NestError::NoRegion
        | NestError::WidenDenied
        | NestError::DramOverlap
        | NestError::Shootdown
        | NestError::Pinned => UnitFail::Denied,
    }
}

fn copy_words(space: &Space, va: KVirt, count: usize) -> Option<Vec<usize>> {
    if count > MAX_ARGS {
        return None;
    }
    let width = size_of::<usize>();
    let len = count * width;
    let mut bytes = [0u8; MAX_ARGS * size_of::<usize>()];
    if !space.copy_in(&mut bytes[..len], va) {
        return None;
    }
    let mut out: Vec<usize> = Vec::new();
    out.try_reserve(count).ok()?;
    for i in 0..count {
        let mut w = [0u8; size_of::<usize>()];
        w.copy_from_slice(&bytes[i * width..(i + 1) * width]);
        out.push(usize::from_le_bytes(w));
    }
    Some(out)
}

use core::sync::atomic::Ordering;
use kernel_api::model::unit::{ControlId, ControlKind, UnitCreate, UnitInfo, UnitQuery, UnitState};
mod inspect;
pub(super) fn dispatch(frame: &mut TrapContext, call: UnitCall, ident: Arc<TaskIdent>) -> Outcome {
    frame.gpr.set_x(Gprs::A0, 0);
    let Some(me) = muster(ident.id).and_then(|w| w.upgrade()) else {
        return Outcome::fail(frame, UnitFail::Denied);
    };
    match call {
        UnitCall::Create { request } => {
            let result = create(&me, request);
            match result {
                Ok(UnitTarget::Task(id)) => {
                    frame.gpr.set_x(Gprs::A0, 0);
                    frame.gpr.set_x(Gprs::A1, id.get());
                }
                Ok(UnitTarget::Team(id)) => {
                    frame.gpr.set_x(Gprs::A0, 1);
                    frame.gpr.set_x(Gprs::A1, id.get());
                }
                Err(error) => return Outcome::fail(frame, error),
            }
        }
        UnitCall::Inspect { query, out } => {
            let result = inspect::query(&me, query).and_then(|info| {
                let Some(info) = info else {
                    return Ok(0);
                };
                if !ident.team.space.copy_out(&info.to_bytes(), out) {
                    return Err(UnitFail::Denied);
                }
                Ok(1)
            });
            super::outcome::answer(frame, result);
        }
        UnitCall::Delete { target } => {
            let result = match target {
                UnitTarget::Team(id) => crate::basis::unit::team::oust(&me, id),
                UnitTarget::Task(_) => (|| {
                    let _commit = crate::basis::commit();
                    let crate::basis::unit::lifecycle::join::JoinTarget::Task(member) =
                        crate::basis::unit::lifecycle::join::observe(&me, target)?
                    else {
                        unreachable!()
                    };
                    if !member.reclaimed.load(Ordering::Acquire) {
                        return Err(UnitFail::Busy);
                    }
                    if !member.receive(me.ident.id) {
                        return Err(UnitFail::Denied);
                    }
                    if let Some(team) = member.node.upgrade() {
                        team.prune();
                    }
                    crate::basis::sign::signal(crate::basis::sign::WakeKey::Task { id: member.id });
                    Ok(())
                })(),
            };
            if let Err(error) = result {
                return Outcome::fail(frame, error);
            }
        }
        UnitCall::Finish { team } => {
            let Some(target) = me.clan(team) else {
                return Outcome::fail(frame, UnitFail::Denied);
            };
            target.finish();
        }
        UnitCall::Accord {
            authority,
            target,
            delegable,
        } => {
            let result = muster(target)
                .and_then(|weak| weak.upgrade())
                .ok_or(UnitFail::Denied)
                .and_then(|target| {
                    crate::basis::unit::control::delegate(&me, &target, authority, delegable)
                })
                .map(ControlId::get);
            super::outcome::answer(frame, result);
        }
        UnitCall::Revoke {
            authority,
            target,
            branch,
        } => {
            let _commit = crate::basis::commit();
            let result = muster(target)
                .and_then(|weak| weak.upgrade())
                .ok_or(UnitFail::Denied)
                .and_then(|target| {
                    me.administration
                        .revoke(authority, &target.administration, branch)
                });
            super::outcome::answer(frame, result.map(|()| 0));
        }
        UnitCall::Return { authority } => {
            let _commit = crate::basis::commit();
            super::outcome::answer(frame, me.administration.release(authority).map(|()| 0));
        }
        UnitCall::Exit {
            value,
            reason,
            note,
            len,
        } => {
            {
                let _commit = crate::basis::commit();
                let mut state = me.state.lock();
                if !state.is_dead() {
                    let hart = state.on_hart();
                    state.doom(
                        hart,
                        crate::basis::unit::task::TaskExitCause::Reap,
                        reason as usize,
                    );
                    drop(state);
                    let member = me
                        .ident
                        .team
                        .life
                        .member(me.ident.id)
                        .expect("published Task member");
                    *member.result.lock() = Some(value as usize);
                }
            }
            crate::basis::unit::lifecycle::set_exit_note(note.get(), len);
            crate::basis::unit::lifecycle::set_exit_reason(reason as usize);
            drop(me);
            drop(ident);
            return Outcome::Exit;
        }
        UnitCall::Resume { target } | UnitCall::Freeze { target } | UnitCall::Stop { target } => {
            let result = match target {
                UnitTarget::Task(id) => {
                    let Some(task) = muster(id).and_then(|w| w.upgrade()) else {
                        return Outcome::fail(frame, UnitFail::Denied);
                    };
                    if !crate::basis::unit::control::manages(&me, &task) {
                        return Outcome::fail(frame, UnitFail::Denied);
                    }
                    match call {
                        UnitCall::Resume { .. } => Task::embark(&task),
                        UnitCall::Freeze { .. } => {
                            let result = Task::debark(&task);
                            if id == ident.id && matches!(result, Err(UnitFail::Busy)) {
                                drop(task);
                                drop(me);
                                drop(ident);
                                return Outcome::Switch(
                                    crate::basis::room::conductor::run::run() as *mut TrapContext
                                );
                            }
                            result
                        }
                        UnitCall::Stop { .. } => {
                            crate::basis::unit::lifecycle::slay(&task);
                            if id == ident.id {
                                drop(task);
                                drop(me);
                                drop(ident);
                                return Outcome::Switch(
                                    crate::basis::unit::lifecycle::quit() as *mut TrapContext
                                );
                            }
                            Ok(())
                        }
                        _ => unreachable!(),
                    }
                }
                UnitTarget::Team(id) => {
                    let Some(team) = me.clan(id) else {
                        return Outcome::fail(frame, UnitFail::Denied);
                    };
                    match call {
                        UnitCall::Resume { .. } => team.embark(),
                        UnitCall::Freeze { .. } => team.debark(),
                        UnitCall::Stop { .. } => {
                            crate::basis::unit::lifecycle::cull(
                                ::core::slice::from_ref(&team),
                                crate::basis::unit::lifecycle::EXIT_DOOM,
                            );
                            Ok(())
                        }
                        _ => unreachable!(),
                    }
                }
            };
            if let Err(error) = result {
                return Outcome::fail(frame, error);
            }
        }
    }
    Outcome::Resume
}
fn create(me: &Arc<Task>, request: KVirt) -> Result<UnitTarget, UnitFail> {
    let mut bytes = [0; UnitCreate::WIDTH];
    if !me.ident.team.space.copy_in(&mut bytes, request) {
        return Err(UnitFail::Denied);
    }
    let spec = UnitCreate::from_bytes(&bytes).ok_or(UnitFail::Denied)?;
    match spec {
        UnitCreate::Task {
            team,
            entry,
            args,
            count,
            stack,
        } => {
            let target = match team {
                None => me.ident.team.clone(),
                Some(team) => me.clan(team).ok_or(UnitFail::Denied)?,
            };
            let words =
                copy_words(&me.ident.team.space, args, count as usize).ok_or(UnitFail::Denied)?;
            let task = crate::basis::unit::team::spawn(
                &target,
                Some(me),
                entry.get(),
                words,
                stack.get(),
            )?;
            Ok(UnitTarget::Task(task.ident.id))
        }
        UnitCreate::Team { kind, guard } => {
            let _commit = crate::basis::commit();
            if !crate::basis::pend::live(me)
                || kind == kernel_api::model::unit::ProgramKind::Supervisor
                    && !me.ident.team.space.kind().is_supervisor()
                || !me.administration.allows(ControlKind::Build)
            {
                return Err(UnitFail::Denied);
            }
            if guard != ControlId::NONE
                && (!me.administration.matches(guard, ControlKind::Guard)
                    || me.ident.team.life.protected())
            {
                return Err(UnitFail::Denied);
            }
            let sire = TaskWeak::stored(Arc::downgrade(me), Origin::Sire);
            let team = crate::basis::unit::team::TeamBuilder::build_with_creator(
                SpaceKind::from(kind),
                sire,
                me.ident.id,
            )
            .map_err(map_err)?;
            // New empty Team: guard selection is immutable and cannot race a member exit.
            if guard != ControlId::NONE {
                crate::basis::unit::team::exit::arm(&team.life, me.ident.id)
                    .expect("new empty guarded Team");
            }
            Ok(UnitTarget::Team(team.id))
        }
    }
}
