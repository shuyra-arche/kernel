//! Queue authority, payload capture and atomic recipient publication.
use alloc::sync::Arc;

use kernel_api::abi::call::clone_view::CloneFail;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::model::clone_view::CloneLimits;
use kernel_api::model::clone_view::PullOutcome;
use kernel_api::model::identity::Mark;
use kernel_api::model::identity::PendId;
use kernel_api::model::identity::TaskId;
use kernel_api::model::pend::{CloneRights, Operations};

use crate::basis::nest::Access;
use crate::basis::pend;
use crate::basis::pend::Modifiable;
use crate::basis::pend::Need;
use crate::basis::pend::access as authority;
use crate::basis::room::scheduler::slot::current;
use crate::basis::unit::task::TaskIdent;
use crate::basis::view;
use crate::basis::view::View;
use kernel_api::address::VirtAddr as KVirt;

pub(crate) fn push(
    ident: Arc<TaskIdent>,
    token: PendId,
    msg: usize,
    len: usize,
) -> Result<(), CloneFail> {
    let me = current()
        .running_task()
        .map(|t| t.ident.id)
        .unwrap_or(TaskId::new(0));
    with_clone(token, Need::Write, CloneRights::PUSH, |meta| {
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
    })
}

pub(crate) fn receive(token: PendId, max_len: usize) -> Result<PullOutcome, CloneFail> {
    let task = current().running_task().ok_or(CloneFail::Denied)?;
    let observed = authority::accede(token, Need::Read).map_err(fail)?;
    authority::usable(&observed).map_err(fail)?;
    if !observed.permits(Operations::Clone(CloneRights::PULL)) {
        return Err(CloneFail::Denied);
    }
    let meta = observed.target().clone_view().ok_or(CloneFail::Denied)?;
    let mut reading = view::clone::read(&meta)?;
    let len = reading.bytes.len();
    if len > max_len {
        return Err(CloneFail::Denied);
    }
    let delivered = alloc::sync::Arc::try_new(reading.bytes.prepare(task.ident.id).map_err(fail)?)
        .map_err(|_| CloneFail::OutOfMemory)?;
    let outcome = PullOutcome {
        token: delivered.token(),
        len,
        sender: reading.from,
    };
    {
        let _serial = task.pend.lock();
        let registration = task.pend.prepare(delivered).map_err(fail)?;
        let _commit = crate::basis::commit();
        if !pend::live(&task) {
            return Err(CloneFail::Dead);
        }
        let current = pend::locate(&task, token).ok_or(CloneFail::Denied)?;
        if !current.target().same(observed.target())
            || !current.allows(Need::Read)
            || !current.permits(Operations::Clone(CloneRights::PULL))
        {
            return Err(CloneFail::Denied);
        }
        registration.commit(|publish| {
            reading.commit(|| {
                publish();
                pend::changed(&task);
            })
        })?;
    }
    // Reading wakes queue observers after all publication locks are released.
    Ok(outcome)
}

pub(crate) fn peek(token: PendId) -> Result<(usize, TaskId, usize), CloneFail> {
    with_clone(token, Need::Read, CloneRights::PEEK, |meta| {
        view::clone::peek(meta)
    })
}
/// 取这一枚 Clone 并按本域词汇报错：失败不是 Clone 的状态就是权柄轴的状态。
fn with_clone<T>(
    token: PendId,
    need: Need,
    rights: CloneRights,
    operation: impl FnOnce(&Arc<view::clone::CloneView>) -> Result<T, CloneFail>,
) -> Result<T, CloneFail> {
    let task = current().running_task().ok_or(CloneFail::Denied)?;
    let _serial = task.pend.lock();
    let pend = authority::accede(token, need).map_err(fail)?;
    authority::usable(&pend).map_err(fail)?;
    if !pend.permits(Operations::Clone(rights)) {
        return Err(CloneFail::Denied);
    }
    let meta = pend.target().clone_view().ok_or(CloneFail::Denied)?;
    operation(&meta)
}

pub(crate) fn create(license: PendId, mark: Mark, limits: CloneLimits) -> Result<usize, PendFail> {
    let task = current().running_task().ok_or(PendFail::Denied)?;
    let _serial = task.pend.lock();
    authority::license(
        &task,
        license,
        kernel_api::model::pend::CreationRights::CLONE,
    )?;
    if !limits.valid() {
        return Err(PendFail::Denied);
    }
    let meta = view::clone::CloneView::try_new_with_limits(task.ident.id, limits)
        .map_err(|_| PendFail::OutOfMemory)?;
    let pend = pend::PendEntry::try_new(View::Clone(meta), mark, Modifiable::Yes, None)?;
    let token = pend.token();
    let entry = Arc::try_new(pend).map_err(|_| PendFail::OutOfMemory)?;
    let _commit = crate::basis::commit();
    if !pend::live(&task) {
        return Err(PendFail::Dead);
    }
    task.pend.prepare(entry)?.publish();
    pend::changed(&task);
    Ok(token.get())
}

pub(crate) fn fail(error: PendFail) -> CloneFail {
    match error {
        PendFail::Denied | PendFail::NotAligned | PendFail::Invalid => CloneFail::Denied,
        PendFail::Dead => CloneFail::Dead,
        PendFail::OutOfMemory => CloneFail::OutOfMemory,
        PendFail::HandedOver => CloneFail::HandedOver,
        PendFail::Busy => CloneFail::Busy,
    }
}
