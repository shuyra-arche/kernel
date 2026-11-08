use alloc::sync::Arc;

use kernel_api::abi::call::pend::PendCall;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::address::VirtAddr;
use kernel_api::model::identity::PendId;
use kernel_api::model::identity::TaskId;
use kernel_api::model::pend::PendInfo;
use kernel_api::model::pend::{AccordRequest, AccordTarget};

use crate::basis::nest::Access;
use crate::basis::pend;
use crate::basis::pend::Modifiable;
use crate::basis::pend::PendSnapshot;
use crate::basis::room::conductor::current;
use crate::basis::room::scheduler::roster::muster;
use crate::basis::unit::task::TaskIdent;
use crate::machine::switcher::context::TrapContext;
use kernel_api::address::VirtAddr as KVirt;

use super::outcome::answer;

pub(crate) fn dispatch(frame: &mut TrapContext, call: PendCall, ident: Arc<TaskIdent>) {
    match call {
        PendCall::Accord { request } => accord(frame, &ident, request),
        PendCall::Narrow { token, subset } => narrow(frame, token, subset),
        PendCall::Revoke { dst, token } => revoke(frame, dst, token),
        PendCall::Collect {
            after,
            buf,
            capacity,
        } => collect(frame, &ident, after, buf, capacity),
        PendCall::Inspect { token, buf } => inspect(frame, &ident, token, buf),
        PendCall::Return { token } => return_(frame, token),
    }
}

fn accord(frame: &mut TrapContext, ident: &TaskIdent, request: VirtAddr) {
    let r = (|| -> Result<usize, PendFail> {
        // Copy once; authorization never rereads mutable user memory.
        let mut bytes = [0; AccordRequest::WIDTH];
        if !ident.team.space.copy_in(&mut bytes, request) {
            return Err(PendFail::Denied);
        }
        let request = AccordRequest::from_bytes(&bytes).ok_or(PendFail::Invalid)?;
        let caller = current().running_task().ok_or(PendFail::Denied)?;
        let target = match request.target {
            AccordTarget::Caller => Arc::downgrade(&caller),
            AccordTarget::Task(id) => muster(id).ok_or(PendFail::Denied)?.as_weak().clone(),
        };
        pend::accord(
            &caller,
            request.src,
            &target,
            request.subset,
            request.mark,
            pend::Window::Parent(request.range),
        )
    })();
    answer(frame, r);
}

fn narrow(frame: &mut TrapContext, token: PendId, subset: Modifiable) {
    let r = current()
        .running_task()
        .ok_or(PendFail::Denied)
        .and_then(|task| pend::reduce(&task, token, subset))
        .map(|()| 0);
    answer(frame, r);
}

fn revoke(frame: &mut TrapContext, dst_id: TaskId, token: PendId) {
    let r = (|| -> Result<usize, PendFail> {
        let caller = current().running_task().ok_or(PendFail::Denied)?;
        let target = muster(dst_id).ok_or(PendFail::Denied)?;
        pend::revoke(&caller, target.as_weak(), token).map(|_| 0)
    })();
    answer(frame, r);
}

fn information(task: &Arc<crate::basis::unit::task::Task>, pend: &PendSnapshot) -> PendInfo {
    PendInfo {
        token: pend.token(),
        view: kernel_api::model::view::ViewId::new(pend.view().id()),
        kind: pend.view().kind(),
        modifiable: pend.modifiable(),
        owner: pend.view().owner(),
        lender: if pend.sire().is_some() {
            pend::vestor(task, pend.token()).unwrap_or(TaskId::new(0))
        } else {
            TaskId::new(0)
        },
        mark: pend.mark(),
        status: if pend.view().alive() {
            pend.authority().status()
        } else {
            kernel_api::model::pend::PendStatus::Dead
        },
    }
}

fn write_info(ident: &TaskIdent, buf: usize, info: PendInfo) -> Result<(), PendFail> {
    let words = info.words();
    let bytes =
        unsafe { ::core::slice::from_raw_parts(words.as_ptr().cast::<u8>(), PendInfo::SIZE) };
    if !ident.team.space.copy_out(bytes, KVirt::wrap(buf)) {
        return Err(PendFail::Denied);
    }
    Ok(())
}

fn inspect(frame: &mut TrapContext, ident: &TaskIdent, token: PendId, buf: VirtAddr) {
    let r = (|| -> Result<usize, PendFail> {
        let task = current().running_task().ok_or(PendFail::Denied)?;
        let pend = pend::locate(&task, token).ok_or(PendFail::Denied)?;
        if !ident
            .team
            .space
            .validate_access(KVirt::wrap(buf.get()), PendInfo::SIZE, Access::WRITE)
        {
            return Err(PendFail::Denied);
        }
        write_info(ident, buf.get(), information(&task, &pend))?;
        Ok(0)
    })();
    answer(frame, r);
}

fn collect(
    frame: &mut TrapContext,
    ident: &TaskIdent,
    after: PendId,
    buf: VirtAddr,
    capacity: usize,
) {
    let r = (|| -> Result<usize, PendFail> {
        let task = current().running_task().ok_or(PendFail::Denied)?;
        let len = capacity
            .checked_mul(PendInfo::SIZE)
            .ok_or(PendFail::Denied)?;
        if capacity == 0 {
            return Ok(0);
        }
        if !ident
            .team
            .space
            .validate_access(KVirt::wrap(buf.get()), len, Access::WRITE)
        {
            return Err(PendFail::Denied);
        }
        let capacity = capacity.min(16);
        let mut snapshots: [Option<PendSnapshot>; 16] = ::core::array::from_fn(|_| None);
        let mut count = 0;
        {
            let table = task.pend.entries.lock();
            for pend in table.iter().filter(|p| p.token().get() > after.get()) {
                let at = snapshots[..count]
                    .partition_point(|p| p.as_ref().unwrap().token().get() < pend.token().get());
                if at < capacity {
                    let next = (count + 1).min(capacity);
                    for i in (at + 1..next).rev() {
                        snapshots[i] = snapshots[i - 1].take();
                    }
                    snapshots[at] = Some(pend.snapshot());
                    count = next;
                }
            }
        }
        for (i, pend) in snapshots[..count].iter().enumerate() {
            write_info(
                ident,
                buf.get() + i * PendInfo::SIZE,
                information(&task, pend.as_ref().unwrap()),
            )?;
        }
        Ok(count)
    })();
    answer(frame, r);
}

fn return_(frame: &mut TrapContext, token: PendId) {
    let r = current()
        .running_task()
        .ok_or(PendFail::Denied)
        .and_then(|task| pend::return_(&task, token))
        .map(|_| 0);
    answer(frame, r);
}
