use alloc::sync::Arc;

use kernel_api::{
    abi::call::clone_view::CloneCall, abi::call::clone_view::CloneFail, abi::call::pend::PendFail,
    model::clone_view::CloneCondition, model::clone_view::CloneLimits, model::clone_view::Oversize,
    model::clone_view::PullOutcome, model::identity::Mark, model::identity::PendId,
    model::identity::TaskId, model::view::ViewKind, model::wait::Wait,
};

use super::gate;
use super::outcome::{Outcome, answer, ret_err};
use crate::core::memory::address::VirtAddr as KVirt;
use crate::core::memory::nest::Access;
use crate::core::work::pend::{self, Modifiable, Need};
use crate::core::work::room::Handoff;
use crate::core::work::room::conductor::current;
use crate::core::work::unit::space::Space;
use crate::core::work::unit::task::TaskIdent;
use crate::core::work::view::{self, View};
use crate::machine::switcher::context::{Gprs, TrapContext};

pub(super) fn dispatch(frame: &mut TrapContext, call: CloneCall, ident: Arc<TaskIdent>) -> Outcome {
    match call {
        CloneCall::Unseal { mark, limits } => {
            answer(frame, create(mark, limits).map_err(fail));
            Outcome::Resume
        }
        CloneCall::Seal { token } => {
            answer(
                frame,
                super::seal::seal_resource(token, ViewKind::Clone).map_err(fail),
            );
            Outcome::Resume
        }
        CloneCall::Push { token, msg, len } => push(frame, ident, token, msg.get(), len),
        CloneCall::Pull {
            token,
            buf,
            max,
            oversize,
        } => pull(frame, ident, token, buf.get(), max, oversize),
        CloneCall::Peek { token } => peek(frame, token),
        CloneCall::Wait {
            token,
            condition,
            millis,
        } => wait(frame, ident, token, condition, millis),
    }
}

fn push(
    frame: &mut TrapContext,
    ident: Arc<TaskIdent>,
    token: PendId,
    msg: usize,
    len: usize,
) -> Outcome {
    let me = current()
        .running_task()
        .map(|t| t.ident.id)
        .unwrap_or(TaskId::new(0));
    let r = with_clone(token, Need::Write, |meta| {
        if len == 0
            || !ident
                .team
                .space
                .validate_access(KVirt::wrap(msg), len, Access::READ)
        {
            return Err(CloneFail::Denied);
        }
        let reservation = view::clone_view::reserve_len(meta, len)?;
        let bytes = view::clone_view::payload::Payload::capture(&ident.team.space, msg, len)?;
        reservation.commit(bytes, me)
    });
    answer(frame, r.map(|()| 0));
    Outcome::Resume
}

fn pull(
    frame: &mut TrapContext,
    ident: Arc<TaskIdent>,
    token: PendId,
    buf: usize,
    max: usize,
    oversize: Oversize,
) -> Outcome {
    if !Space::user_range(KVirt::wrap(buf), max) {
        ret_err(frame, CloneFail::Denied);
        return Outcome::Resume;
    }
    let r = with_clone(token, Need::Read, |meta| {
        if !ident
            .team
            .space
            .validate_access(KVirt::wrap(buf), max, Access::WRITE)
        {
            return Err(CloneFail::Denied);
        }
        hand_over(meta, &ident.team.space, buf, max, oversize)
    });
    match r {
        Ok(outcome) => {
            let (n, from, discarded) = match outcome {
                PullOutcome::Received { len, sender } => (len, sender, false),
                PullOutcome::Discarded { len, sender } => (len, sender, true),
            };
            frame.gpr.set_x(Gprs::A0, n);
            frame.gpr.set_x(Gprs::A1, from.get());
            frame.gpr.set_x(Gprs::A2, discarded as usize);
        }
        Err(e) => ret_err(frame, e),
    }
    Outcome::Resume
}

fn hand_over(
    meta: &Arc<view::clone_view::CloneView>,
    space: &Arc<Space>,
    buf: usize,
    max: usize,
    oversize: Oversize,
) -> Result<PullOutcome, CloneFail> {
    let reading = view::clone_view::read(meta)?;
    let from = reading.from;
    let len = reading.bytes.len();
    if len > max && oversize == Oversize::Discard {
        reading.finish();
        return Ok(PullOutcome::Discarded { len, sender: from });
    }
    if len > max || !space.validate_access(KVirt::wrap(buf), len, Access::WRITE) {
        view::clone_view::note_back(len, max);
        return Err(CloneFail::Denied);
    }
    if !reading.bytes.copy_out(space, buf) {
        view::clone_view::note_back(len, max);
        return Err(CloneFail::Denied);
    }
    reading.finish();
    Ok(PullOutcome::Received { len, sender: from })
}

fn peek(frame: &mut TrapContext, token: PendId) -> Outcome {
    let r = with_clone(token, Need::Read, |meta| view::clone_view::peek(meta));
    match r {
        Ok((n, from, depth)) => {
            frame.gpr.set_x(Gprs::A0, n);
            frame.gpr.set_x(Gprs::A1, from.get());
            frame.gpr.set_x(Gprs::A2, depth);
        }
        Err(e) => ret_err(frame, e),
    }
    Outcome::Resume
}

fn wait(
    frame: &mut TrapContext,
    ident: Arc<TaskIdent>,
    token: PendId,
    condition: CloneCondition,
    millis: Wait,
) -> Outcome {
    let dur = millis.into_duration();
    let r = with_clone(token, view::clone_view::need(condition), |meta| {
        frame.gpr.set_x(Gprs::A0, 0);
        drop(ident);
        view::clone_view::wait(meta, condition, dur)
    });
    match r {
        Ok(Handoff::Resume(ready)) => frame.gpr.set_x(Gprs::A0, ready as usize),
        Ok(Handoff::Switch(pa)) => return Outcome::Switch(pa as *mut TrapContext),
        Err(e) => ret_err(frame, e),
    }
    Outcome::Resume
}

/// 取这一枚 Clone 并按本域词汇报错：失败不是 Clone 的状态就是权柄轴的状态。
fn with_clone<T>(
    token: PendId,
    need: Need,
    operation: impl FnOnce(&Arc<view::clone_view::CloneView>) -> Result<T, CloneFail>,
) -> Result<T, CloneFail> {
    let pend = gate::accede(token, need).map_err(fail)?;
    gate::usable(&pend).map_err(fail)?;
    let meta = pend.view().clone_view().ok_or(CloneFail::Denied)?;
    operation(&meta)
}

pub(crate) fn create(mark: Mark, limits: CloneLimits) -> Result<usize, PendFail> {
    let task = current().running_task().ok_or(PendFail::Denied)?;
    if !limits.valid() {
        return Err(PendFail::Denied);
    }
    let meta = view::clone_view::CloneView::try_new_with_limits(task.ident.id, limits)
        .map_err(|_| PendFail::OoM)?;
    let pend = pend::PendEntry::try_new(View::Clone(meta), mark, Modifiable::Yes, None)?;
    let token = pend.token();
    pend::insert(&task, pend)?;
    Ok(token.get())
}

fn fail(error: PendFail) -> CloneFail {
    match error {
        PendFail::Denied | PendFail::NotAligned => CloneFail::Denied,
        PendFail::Dead => CloneFail::Dead,
        PendFail::OoM => CloneFail::OoM,
        PendFail::HandedOver => CloneFail::HandedOver,
        PendFail::Busy => CloneFail::Busy,
    }
}
