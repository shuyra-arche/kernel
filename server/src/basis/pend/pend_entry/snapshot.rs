use alloc::sync::Arc;
use alloc::sync::Weak;
use kernel_api::model::identity::Mark;
use kernel_api::model::identity::PendId;
use kernel_api::model::identity::TaskId;

use crate::basis::pend::authority::Pend;
use crate::basis::unit::task::Task;
use crate::basis::view::View;

use super::super::Need;
use super::Modifiable;
use super::PendEntry;

#[derive(Clone)]
pub(crate) struct PendSnapshot {
    pub(super) token: PendId,
    pub(super) authority: Arc<Pend>,
    pub(super) mark: Mark,
    pub(super) sire: Option<PendId>,
    pub(super) lord: Weak<Task>,
    pub(super) meta: View,
}

impl PendSnapshot {
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
