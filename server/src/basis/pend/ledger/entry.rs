use crate::basis::pend::Pend;

use alloc::sync::Arc;
use alloc::sync::Weak;

use kernel_api::model::identity::Mark;
use kernel_api::model::identity::PendId;
use kernel_api::model::identity::TaskId;

use crate::basis::pend::narrows;
use crate::basis::unit::task::Task;
use crate::basis::view::View;

use crate::basis::pend::{Modifiable, Need};

pub(crate) struct PendEntry {
    released: core::sync::atomic::AtomicBool,
    token: PendId,
    authority: Arc<Pend>,
    mark: Mark,
    sire: Option<PendId>,
    lord: Weak<Task>,
    meta: View,
}

impl PendEntry {
    /// 这一枚 PendEntry 的 Pend：持有它的调用方据此知道自己的分支在哪条撤销边上。
    pub(crate) fn authority(&self) -> &Arc<Pend> {
        &self.authority
    }

    pub(crate) fn token(&self) -> PendId {
        self.token
    }

    pub(crate) fn modifiable(&self) -> Modifiable {
        self.authority.configured()
    }

    pub(crate) fn sire(&self) -> Option<PendId> {
        self.sire
    }

    pub(crate) fn lord(&self) -> &Weak<Task> {
        &self.lord
    }

    pub(crate) fn mark(&self) -> Mark {
        self.mark
    }

    pub(crate) fn owner(&self) -> Option<TaskId> {
        self.alive().then(|| self.meta.owner())
    }

    pub(crate) fn alive(&self) -> bool {
        self.meta.alive() && self.authority.alive()
    }

    pub(crate) fn allows(&self, need: Need) -> bool {
        self.authority
            .modifiable()
            .is_some_and(|access| need.allowed_by(access))
    }

    pub(crate) fn view(&self) -> &View {
        &self.meta
    }

    /// 同 `vest`，但给这一枚子分支一个**有效窗口**（相对来源起点的绝对页偏移与页数）。
    pub(crate) fn vest_window(
        &self,
        mark: Mark,
        modifiable: Modifiable,
        window: Option<(
            kernel_api::address::PageOffset,
            kernel_api::address::PageCount,
        )>,
    ) -> Result<PendEntry, kernel_api::abi::call::pend::PendFail> {
        PendEntry::try_new_window(
            self.meta.clone(),
            mark,
            modifiable,
            Some(self.token),
            window,
        )
    }
}

impl PendEntry {
    pub(crate) fn try_new(
        meta: View,
        mark: Mark,
        modifiable: Modifiable,
        sire: Option<PendId>,
    ) -> Result<PendEntry, kernel_api::abi::call::pend::PendFail> {
        Self::try_new_window(meta, mark, modifiable, sire, None)
    }

    /// 带**有效窗口**的一枚：窗口相对来源起点，只在建这一枚时给定。
    pub(crate) fn try_new_window(
        meta: View,
        mark: Mark,
        modifiable: Modifiable,
        sire: Option<PendId>,
        window: Option<(
            kernel_api::address::PageOffset,
            kernel_api::address::PageCount,
        )>,
    ) -> Result<PendEntry, kernel_api::abi::call::pend::PendFail> {
        let token = next_id()?;
        Ok(PendEntry {
            released: core::sync::atomic::AtomicBool::new(false),
            token,
            authority: meta.new_pend(modifiable, token, window)?,
            mark,
            sire,
            lord: Weak::new(),
            meta,
        })
    }

    pub(super) fn release(&self) {
        if !self
            .released
            .swap(true, core::sync::atomic::Ordering::AcqRel)
        {
            self.meta.release_pend(self.token);
        }
    }

    pub(crate) fn invalidate(&self) {
        self.authority.invalidate();
    }

    pub(crate) fn parent(&mut self, token: PendId, task: Weak<Task>) {
        self.sire = Some(token);
        self.lord = task;
    }

    pub(crate) fn narrow(
        &self,
        subset: Modifiable,
        window: Option<(
            kernel_api::address::PageOffset,
            kernel_api::address::PageCount,
        )>,
    ) -> Result<(), kernel_api::abi::call::pend::PendFail> {
        if !self.alive() {
            return Err(kernel_api::abi::call::pend::PendFail::Dead);
        }
        if !narrows(self.modifiable(), subset) {
            return Err(kernel_api::abi::call::pend::PendFail::Denied);
        }
        self.authority.narrow(subset, window)
    }
}

impl Drop for PendEntry {
    fn drop(&mut self) {
        self.authority.invalidate();
        self.release();
    }
}

// SAFETY: identity fields are immutable once shared; authority and View state
// use their own locks/atomics. PendId's !Send marker protects API ownership, not
// this kernel registration, which is always accessed under ledger/COMMIT rules.
unsafe impl Send for PendEntry {}
unsafe impl Sync for PendEntry {}
fn next_id() -> Result<PendId, kernel_api::abi::call::pend::PendFail> {
    static NEXT: crate::basis::name::Serial = crate::basis::name::Serial::new(1, usize::MAX);
    NEXT.allocate()
        .map(PendId::mint)
        .ok_or(kernel_api::abi::call::pend::PendFail::OutOfMemory)
}

pub(crate) fn locate(task: &Task, token: PendId) -> Option<Arc<PendEntry>> {
    let entries = task.pend.entries.lock();
    entries.get(token).cloned()
}

pub(crate) fn vestor(task: &Arc<Task>, token: PendId) -> Option<TaskId> {
    for _ in 0..super::RETRIES {
        let observed = {
            let _pend = task.pend.lock();
            locate(task, token)?
        };
        let parent = observed.sire()?;
        let lord = observed.lord().upgrade()?;
        let result = super::with_pair(task, &lord, || {
            let Some(current) = locate(task, token) else {
                return Some(None);
            };
            if current.sire() != Some(parent) || !current.lord().ptr_eq(observed.lord()) {
                return None;
            }
            Some(locate(&lord, parent).map(|_| lord.ident.id))
        });
        if let Some(result) = result {
            return result;
        }
    }
    None
}

pub(crate) fn accede(
    task: &Arc<Task>,
    token: PendId,
    need: Need,
) -> Result<Arc<PendEntry>, kernel_api::abi::call::pend::PendFail> {
    let pend_entry = locate(task, token).ok_or(kernel_api::abi::call::pend::PendFail::Denied)?;
    if !pend_entry.alive() {
        return Err(kernel_api::abi::call::pend::PendFail::Dead);
    }
    if !pend_entry.allows(need) {
        return Err(kernel_api::abi::call::pend::PendFail::Denied);
    }
    Ok(pend_entry)
}
