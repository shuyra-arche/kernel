use alloc::sync::Arc;

use kernel_api::{
    abi::call::pend::PendCall, abi::call::pend::PendFail, model::identity::Mark,
    model::identity::PendId, model::identity::TaskId, model::identity::VirtAddr,
    model::pend::PendInfo, model::pend::PendRange, model::pend::ReleaseMode,
};

use crate::basis::memory::address::VirtAddr as KVirt;
use crate::basis::memory::address::{PageCount, PageOffset};
use crate::basis::memory::nest::Access;
use crate::basis::work::pend::{self, Modifiable, PendSnapshot};
use crate::basis::work::room::conductor::current;
use crate::basis::work::room::scheduler::roster::muster;
use crate::basis::work::unit::task::TaskIdent;
use crate::machine::switcher::context::TrapContext;

use super::outcome::answer;

pub(crate) fn dispatch(frame: &mut TrapContext, call: PendCall, ident: Arc<TaskIdent>) {
    match call {
        PendCall::Accord {
            src,
            dst,
            subset,
            mark,
        } => accord(frame, src, dst, subset, mark),
        PendCall::Narrow { token, subset } => narrow(frame, token, subset),
        PendCall::Revoke { dst, token } => revoke(frame, dst, token),
        PendCall::Collect {
            after,
            buf,
            capacity,
        } => collect(frame, &ident, after, buf, capacity),
        PendCall::Inspect { token, buf } => inspect(frame, &ident, token, buf),
        PendCall::Release { token, mode } => release(frame, token, mode),
        PendCall::Same { a, b } => {
            answer(
                frame,
                current()
                    .running_task()
                    .ok_or(PendFail::Denied)
                    .and_then(|task| pend::same(&task, a, b))
                    .map(usize::from),
            );
        }
        PendCall::Derive {
            src,
            subset,
            mark,
            range,
        } => derive(frame, src, subset, mark, range),
    }
}

fn accord(
    frame: &mut TrapContext,
    src_token: PendId,
    dst_id: TaskId,
    subset: Modifiable,
    mark: Mark,
) {
    let r = (|| -> Result<usize, PendFail> {
        let caller = current().running_task().ok_or(PendFail::Denied)?;
        let src = pend::locate(&caller, src_token).ok_or(PendFail::Denied)?;
        if !src.alive() {
            return Err(PendFail::Dead);
        }
        let dst = muster(dst_id).ok_or(PendFail::Denied)?;
        pend::accord(&caller, src_token, dst.as_weak(), subset, mark)
    })();
    answer(frame, r);
}

/// 本地精确派生：目标就是调用者，范围继承父窗口或按来源起点收窄。
fn derive(
    frame: &mut TrapContext,
    src_token: PendId,
    subset: Modifiable,
    mark: Mark,
    range: PendRange,
) {
    let r = (|| -> Result<usize, PendFail> {
        let window = match range {
            PendRange::Inherit => None,
            PendRange::Pages { offset, count } => {
                let count = PageCount::new(count).ok_or(PendFail::NotAligned)?;
                Some((PageOffset::new(offset), count))
            }
        };
        let caller = current().running_task().ok_or(PendFail::Denied)?;
        let src = pend::locate(&caller, src_token).ok_or(PendFail::Denied)?;
        if !src.alive() {
            return Err(PendFail::Dead);
        }
        pend::derive(&caller, src_token, subset, mark, window)
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

fn information(task: &Arc<crate::basis::work::unit::task::Task>, pend: &PendSnapshot) -> PendInfo {
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

fn release(frame: &mut TrapContext, token: PendId, mode: ReleaseMode) {
    let r = current()
        .running_task()
        .ok_or(PendFail::Denied)
        .and_then(|task| match mode {
            ReleaseMode::Revoke => pend::release(&task, token).map(|_| ()),
        })
        .map(|_| 0);
    answer(frame, r);
}
