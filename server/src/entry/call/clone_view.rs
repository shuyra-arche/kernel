use alloc::sync::Arc;

use kernel_api::abi::call::clone_view::CloneCall;
use kernel_api::abi::call::clone_view::CloneFail;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::model::clone_view::CloneLimits;
use kernel_api::model::clone_view::PullOutcome;
use kernel_api::model::identity::Mark;
use kernel_api::model::identity::PendId;
use kernel_api::model::identity::TaskId;
use kernel_api::model::view::ViewKind;

use super::authority;
use super::outcome::Outcome;
use super::outcome::answer;
use super::outcome::ret_err;
use crate::basis::nest::Access;
use crate::basis::pend;
use crate::basis::pend::Modifiable;
use crate::basis::pend::Need;
use crate::basis::room::conductor::current;
use crate::basis::unit::task::TaskIdent;
use crate::basis::view;
use crate::basis::view::View;
use crate::machine::switcher::context::Gprs;
use crate::machine::switcher::context::TrapContext;
use kernel_api::address::VirtAddr as KVirt;

pub(super) fn dispatch(frame: &mut TrapContext, call: CloneCall, ident: Arc<TaskIdent>) -> Outcome {
    match call {
        CloneCall::Create { mark, limits } => {
            answer(frame, create(mark, limits).map_err(fail));
            Outcome::Resume
        }
        CloneCall::Delete { token } => {
            answer(
                frame,
                authority::seal_resource(token, ViewKind::Clone).map_err(fail),
            );
            Outcome::Resume
        }
        CloneCall::Push { token, msg, len } => push(frame, ident, token, msg.get(), len),
        CloneCall::Pull { token, max_len } => pull(frame, token, max_len),
        CloneCall::Peek { token } => peek(frame, token),
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
                .nest
                .validate_access(KVirt::wrap(msg), len, Access::READ)
        {
            return Err(CloneFail::Denied);
        }
        let reservation = view::clone::reserve_len(meta, len)?;
        let bytes = view::clone::payload::Payload::capture(&ident.team.nest, msg, len)?;
        reservation.commit(bytes, me)
    });
    answer(frame, r.map(|()| 0));
    Outcome::Resume
}

fn pull(frame: &mut TrapContext, token: PendId, max_len: usize) -> Outcome {
    let result = receive(token, max_len);
    match result {
        Ok(delivery) => {
            frame.gpr.set_x(Gprs::A0, delivery.token.get());
            frame.gpr.set_x(Gprs::A1, delivery.len);
            frame.gpr.set_x(Gprs::A2, delivery.sender.get());
        }
        Err(error) => ret_err(frame, error),
    }
    Outcome::Resume
}

fn receive(token: PendId, max_len: usize) -> Result<PullOutcome, CloneFail> {
    let task = current().running_task().ok_or(CloneFail::Denied)?;
    let observed = authority::accede(token, Need::Read).map_err(fail)?;
    authority::usable(&observed).map_err(fail)?;
    let meta = observed.view().clone_view().ok_or(CloneFail::Denied)?;
    let mut reading = view::clone::read(&meta)?;
    let len = reading.bytes.len();
    if len > max_len {
        return Err(CloneFail::Denied);
    }
    let delivered = reading.bytes.prepare(task.ident.id).map_err(fail)?;
    let outcome = PullOutcome {
        token: delivered.token(),
        len,
        sender: reading.from,
    };
    {
        let _serial = task.pend.lock();
        task.pend
            .entries
            .lock()
            .try_reserve(1)
            .map_err(|_| CloneFail::OoM)?;
        let _commit = crate::basis::commit();
        if !pend::live(&task) {
            return Err(CloneFail::Dead);
        }
        let current = pend::locate(&task, token).ok_or(CloneFail::Denied)?;
        if !current.view().same(observed.view()) || !current.allows(Need::Read) {
            return Err(CloneFail::Denied);
        }
        let mut entries = task.pend.entries.lock();
        reading.commit(|| {
            entries.push(delivered);
            pend::changed(&task);
        })?;
    }
    // Reading wakes queue observers after all publication locks are released.
    Ok(outcome)
}

fn peek(frame: &mut TrapContext, token: PendId) -> Outcome {
    let r = with_clone(token, Need::Read, |meta| view::clone::peek(meta));
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

/// 取这一枚 Clone 并按本域词汇报错：失败不是 Clone 的状态就是权柄轴的状态。
fn with_clone<T>(
    token: PendId,
    need: Need,
    operation: impl FnOnce(&Arc<view::clone::CloneView>) -> Result<T, CloneFail>,
) -> Result<T, CloneFail> {
    let pend = authority::accede(token, need).map_err(fail)?;
    authority::usable(&pend).map_err(fail)?;
    let meta = pend.view().clone_view().ok_or(CloneFail::Denied)?;
    operation(&meta)
}

pub(crate) fn create(mark: Mark, limits: CloneLimits) -> Result<usize, PendFail> {
    let task = current().running_task().ok_or(PendFail::Denied)?;
    if !limits.valid() {
        return Err(PendFail::Denied);
    }
    let meta = view::clone::CloneView::try_new_with_limits(task.ident.id, limits)
        .map_err(|_| PendFail::OoM)?;
    let pend = pend::PendEntry::try_new(View::Clone(meta), mark, Modifiable::Yes, None)?;
    let token = pend.token();
    pend::insert(&task, pend)?;
    Ok(token.get())
}

fn fail(error: PendFail) -> CloneFail {
    match error {
        PendFail::Denied | PendFail::NotAligned | PendFail::Invalid => CloneFail::Denied,
        PendFail::Dead => CloneFail::Dead,
        PendFail::OoM => CloneFail::OoM,
        PendFail::HandedOver => CloneFail::HandedOver,
        PendFail::Busy => CloneFail::Busy,
    }
}
