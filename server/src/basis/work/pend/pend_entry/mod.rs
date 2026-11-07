use core::ops::Deref;
use core::sync::atomic::{AtomicUsize, Ordering};

use alloc::sync::{Arc, Weak};

use kernel_api::{model::identity::Mark, model::identity::PendId, model::identity::TaskId};

use crate::basis::work::pend::authority::narrows;
use crate::basis::work::unit::task::Task;
use crate::basis::work::view::View;

use super::Need;

mod snapshot;
pub(crate) use snapshot::PendSnapshot;

pub(crate) struct PendEntry {
    core: PendSnapshot,
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
            crate::basis::memory::address::PageOffset,
            crate::basis::memory::address::PageCount,
        )>,
    ) -> Result<PendEntry, kernel_api::abi::call::pend::PendFail> {
        let token = next_id();
        Ok(PendEntry {
            core: PendSnapshot {
                token,
                authority: meta.new_pend(modifiable, token, window)?,
                mark,
                sire,
                lord: Weak::new(),
                meta,
            },
        })
    }

    pub(crate) fn snapshot(&self) -> PendSnapshot {
        self.core.clone()
    }

    pub(crate) fn invalidate(&self) {
        self.core.authority.invalidate();
    }

    pub(crate) fn parent(&mut self, token: PendId, task: Weak<Task>) {
        self.core.sire = Some(token);
        self.core.lord = task;
    }

    pub(crate) fn narrow(
        &mut self,
        subset: Modifiable,
    ) -> Result<(), kernel_api::abi::call::pend::PendFail> {
        if !self.core.alive() {
            return Err(kernel_api::abi::call::pend::PendFail::Dead);
        }
        if !narrows(self.core.modifiable(), subset) {
            return Err(kernel_api::abi::call::pend::PendFail::Denied);
        }
        self.core.authority.narrow(subset)
    }
}

impl Deref for PendEntry {
    type Target = PendSnapshot;
    fn deref(&self) -> &PendSnapshot {
        &self.core
    }
}

impl Drop for PendEntry {
    fn drop(&mut self) {
        self.core.authority.invalidate();
        self.core.meta.release_pend(self.core.token);
    }
}

unsafe impl Send for PendEntry {}

pub use kernel_api::model::modifiable::Modifiable;

fn next_id() -> PendId {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    PendId::mint(
        NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("PendEntry token identity exhausted"),
    )
}

pub(crate) fn locate(task: &Task, token: PendId) -> Option<PendSnapshot> {
    let entries = task.pend.entries.lock();
    entries
        .iter()
        .find(|p| p.token() == token)
        .map(|p| p.snapshot())
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
) -> Result<PendSnapshot, kernel_api::abi::call::pend::PendFail> {
    let pend_entry = locate(task, token).ok_or(kernel_api::abi::call::pend::PendFail::Denied)?;
    if !pend_entry.alive() {
        return Err(kernel_api::abi::call::pend::PendFail::Dead);
    }
    if !pend_entry.allows(need) {
        return Err(kernel_api::abi::call::pend::PendFail::Denied);
    }
    Ok(pend_entry)
}
