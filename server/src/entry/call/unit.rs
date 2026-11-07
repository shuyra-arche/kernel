use alloc::sync::Arc;
use alloc::vec::Vec;

use kernel_api::{abi::call::unit::UnitCall, abi::call::unit::UnitFail, model::unit::UnitTarget};

use crate::basis::memory::address::VirtAddr as KVirt;
use crate::basis::memory::nest::{NestError, SpaceKind};
use crate::basis::work::room::Handoff;
use crate::basis::work::room::scheduler::roster::muster;
use crate::basis::work::sign::{self};
use crate::basis::work::unit::copies::Origin;
use crate::basis::work::unit::join::JoinWait;
use crate::basis::work::unit::life::TaskLife;
use crate::basis::work::unit::space::Space;
use crate::basis::work::unit::task::{MAX_ARGS, Task, TaskIdent, TaskWeak};
use crate::machine::switcher::context::{Gprs, TrapContext};

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

pub(super) fn dispatch(frame: &mut TrapContext, call: UnitCall, ident: Arc<TaskIdent>) -> Outcome {
    frame.gpr.set_x(Gprs::A0, 0);
    let Some(me) = muster(ident.id).and_then(|w| w.upgrade()) else {
        return Outcome::fail(frame, UnitFail::Denied);
    };
    match call {
        UnitCall::Finish { team } => {
            let Some(target) = me.clan(team) else {
                return Outcome::fail(frame, UnitFail::Denied);
            };
            target.finish();
            frame.gpr.set_x(Gprs::A0, 0);
        }
        UnitCall::Return { value } => {
            {
                let _commit = crate::basis::work::commit();
                let mut state = me.state.lock();
                if !state.is_dead() {
                    state.doom(None, crate::basis::work::unit::task::TaskExitCause::Reap, 0);
                    drop(state);
                    let member = me
                        .ident
                        .team
                        .life
                        .member(me.ident.id)
                        .expect("published Task member");
                    *member.result.lock() = Some(value);
                }
            }
            crate::basis::work::unit::lifecycle::set_exit_note(0, 0);
            crate::basis::work::unit::lifecycle::set_exit_reason(0);
            drop(me);
            drop(ident);
            return Outcome::Exit;
        }
        UnitCall::ReadResult { task } => {
            let result = (|| {
                let _commit = crate::basis::work::commit();
                let selected =
                    crate::basis::work::unit::join::observe(&me, UnitTarget::Task(task))?;
                let crate::basis::work::unit::join::JoinTarget::Task(member) = selected else {
                    unreachable!()
                };
                if !member.pending(me.ident.id) {
                    return Err(UnitFail::Denied);
                }
                if member.exit().is_none() {
                    return Err(UnitFail::Busy);
                }
                Ok(member.result.lock().map_or((0, 0), |value| (1, value)))
            })();
            super::outcome::answer_pair(frame, result);
        }

        UnitCall::DelegateControl {
            authority,
            target,
            delegable,
        } => {
            let result = muster(target)
                .and_then(|weak| weak.upgrade())
                .ok_or(UnitFail::Denied)
                .and_then(|target| {
                    crate::basis::work::unit::control::delegate(&me, &target, authority, delegable)
                });
            match result {
                Ok(id) => frame.gpr.set_x(Gprs::A0, id.get()),
                Err(error) => return Outcome::fail(frame, error),
            }
        }
        UnitCall::ReleaseControl { authority } => {
            let _commit = crate::basis::work::commit();
            match me.administration.release(authority) {
                Ok(()) => frame.gpr.set_x(Gprs::A0, 0),
                Err(error) => return Outcome::fail(frame, error),
            }
        }

        UnitCall::Spawn {
            team,
            entry,
            args,
            count,
            stack,
        } => {
            let target = if team.get() == 0 {
                ident.team.clone()
            } else {
                match me.clan(team) {
                    Some(t) => t,
                    None => return Outcome::fail(frame, UnitFail::Denied),
                }
            };
            let Some(words) = copy_words(&ident.team.space, KVirt::wrap(args.get()), count) else {
                return Outcome::fail(frame, UnitFail::Denied);
            };
            match crate::basis::work::unit::team::spawn(&target, Some(&me), entry, words, stack) {
                Ok(task) => frame.gpr.set_x(Gprs::A0, task.ident.id.get()),
                Err(error) => return Outcome::fail(frame, error),
            }
        }
        UnitCall::Build { kind, sire } => {
            if kind == kernel_api::model::unit::ProgramKind::Supervisor
                && !ident.team.space.kind().is_supervisor()
                || !me
                    .administration
                    .allows(kernel_api::model::unit::ControlKind::Build)
            {
                return Outcome::fail(frame, UnitFail::Denied);
            }
            let Some(parent) = muster(sire).and_then(|w| w.upgrade()) else {
                return Outcome::fail(frame, UnitFail::Denied);
            };
            if parent.dead() || parent.ident.id != me.ident.id {
                return Outcome::fail(frame, UnitFail::Denied);
            }
            let sire = TaskWeak::stored(Arc::downgrade(&parent), Origin::Sire);
            match crate::basis::work::unit::team::TeamBuilder::build_with_creator(
                SpaceKind::from(kind),
                sire,
                me.ident.id,
            ) {
                Ok(team) => frame.gpr.set_x(Gprs::A0, team.id.get()),
                Err(error) => return Outcome::fail(frame, map_err(error)),
            }
        }
        UnitCall::SelfId => frame.gpr.set_x(Gprs::A0, ident.id.get()),
        UnitCall::Creator => frame.gpr.set_x(Gprs::A0, ident.team.creator().get()),
        UnitCall::Sire => frame
            .gpr
            .set_x(Gprs::A0, ident.team.sire().map_or(0, |id| id.get())),
        UnitCall::Scan {
            after,
            buf,
            capacity,
        } => {
            if !(1..=64).contains(&capacity)
                || !Space::user_range(KVirt::wrap(buf.get()), capacity * 8)
            {
                return Outcome::fail(frame, UnitFail::Denied);
            }
            let mut page = [0u64; 64];
            let mut n = 0;
            {
                let _commit = crate::basis::work::commit();
                let heirs = me.heir.lock();
                let mut cursor = after.get();
                while n < capacity {
                    let next = heirs
                        .iter()
                        .map(|t| t.id.get())
                        .filter(|id| *id > cursor)
                        .min();
                    let Some(id) = next else { break };
                    page[n] = id as u64;
                    cursor = id;
                    n += 1;
                }
            }
            let mut bytes = [0u8; 512];
            for (at, id) in page[..n].iter().enumerate() {
                bytes[at * 8..at * 8 + 8].copy_from_slice(&id.to_le_bytes());
            }
            if !ident
                .team
                .space
                .copy_out(&bytes[..n * 8], KVirt::wrap(buf.get()))
            {
                return Outcome::fail(frame, UnitFail::Denied);
            }
            frame.gpr.set_x(Gprs::A0, n);
        }
        UnitCall::Join {
            target,
            millis,
            receive,
        } => {
            if target == UnitTarget::Task(ident.id) && millis != kernel_api::model::wait::Wait::POLL
            {
                return Outcome::fail(frame, UnitFail::Denied);
            }
            let selected = match crate::basis::work::unit::join::observe(&me, target) {
                Ok(target) => target,
                Err(error) => return Outcome::fail(frame, error),
            };
            let join = JoinWait::new(selected, ident.id, receive, millis);
            let caller = ident.id;
            drop(me);
            drop(ident);
            match crate::basis::work::unit::join::wait::wait(join) {
                Ok(Handoff::Resume(reply)) => JoinWait::write(frame, Ok(reply)),
                Ok(Handoff::Switch(pa)) => return Outcome::Switch(pa as *mut TrapContext),
                Err(error) => return Outcome::fail(frame, error),
            }
            {
                let _commit = crate::basis::work::commit();
                if let Some(me) = muster(caller).and_then(|w| w.upgrade()) {
                    me.ident.team.life.prune();
                    let mut at = 0;
                    while let Some(root) = me.heir_node(at) {
                        root.life.prune();
                        at += 1;
                    }
                }
            }
        }
        UnitCall::Oust { team } => {
            if let Err(error) = crate::basis::work::unit::team::oust(&me, team) {
                return Outcome::fail(frame, error);
            }
        }
        UnitCall::Fall { millis } => {
            let mine = TaskLife {
                id: ident.id,
                life: me.life(),
            };
            drop(me);
            drop(ident);
            match sign::fall::<UnitFail>(mine, millis.into_duration()) {
                Ok(Handoff::Resume(landed)) => frame.gpr.set_x(Gprs::A0, landed as usize),
                Ok(Handoff::Switch(pa)) => return Outcome::Switch(pa as *mut TrapContext),
                Err(error) => return Outcome::fail(frame, error),
            }
        }
        UnitCall::Embark { target } | UnitCall::Debark { target } | UnitCall::Slay { target } => {
            let result = match target {
                UnitTarget::Task(id) => {
                    let Some(task) = muster(id).and_then(|w| w.upgrade()) else {
                        return Outcome::fail(frame, UnitFail::Denied);
                    };
                    if !crate::basis::work::unit::control::manages(&me, &task) {
                        return Outcome::fail(frame, UnitFail::Denied);
                    }
                    match call {
                        UnitCall::Embark { .. } => Task::embark(&task),
                        UnitCall::Debark { .. } => {
                            let result = Task::debark(&task);
                            if id == ident.id && matches!(result, Err(UnitFail::Busy)) {
                                drop(task);
                                drop(me);
                                drop(ident);
                                return Outcome::Switch(
                                    crate::basis::work::room::conductor::run::run()
                                        as *mut TrapContext,
                                );
                            }
                            result
                        }
                        UnitCall::Slay { .. } => {
                            crate::basis::work::unit::lifecycle::slay(&task);
                            if id == ident.id {
                                drop(task);
                                drop(me);
                                drop(ident);
                                return Outcome::Switch(
                                    crate::basis::work::unit::lifecycle::quit() as *mut TrapContext,
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
                        UnitCall::Embark { .. } => team.embark(),
                        UnitCall::Debark { .. } => team.debark(),
                        UnitCall::Slay { .. } => {
                            crate::basis::work::unit::lifecycle::cull(
                                ::core::slice::from_ref(&team),
                                crate::basis::work::unit::lifecycle::EXIT_DOOM,
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
        UnitCall::Guard { team, authority } => {
            let target = match me.clan(team) {
                Some(target) => target,
                None if team == ident.team.id => ident.team.clone(),
                None => return Outcome::fail(frame, UnitFail::Denied),
            };
            if !crate::basis::work::unit::team::exit::authorize(me.as_ref(), authority) {
                return Outcome::fail(frame, UnitFail::Denied);
            }
            if let Err(error) = target.guard(ident.id) {
                return Outcome::fail(frame, error);
            }
        }
        UnitCall::Unguard { team, authority } => {
            let target = match me.clan(team) {
                Some(target) => target,
                None if team == ident.team.id => ident.team.clone(),
                None => return Outcome::fail(frame, UnitFail::Denied),
            };
            if !crate::basis::work::unit::team::exit::authorize(me.as_ref(), authority) {
                return Outcome::fail(frame, UnitFail::Denied);
            }
            if let Err(error) = target.unguard(ident.id) {
                return Outcome::fail(frame, error);
            }
        }
    }
    Outcome::Resume
}
