use alloc::sync::Arc;

use kernel_api::{
    abi::call::pie::PieFail, model::identity::PieToken, model::identity::TaskId,
    model::memory::ViewControl,
};

use crate::machine::sync::SpinLock;

pub(crate) struct MappingControlRecord {
    scope: ViewControl,
    control: SpinLock<(TaskId, Option<PieToken>)>,
}

impl MappingControlRecord {
    pub(crate) fn new(
        controller: TaskId,
        scope: ViewControl,
        branch: Option<PieToken>,
    ) -> Result<Arc<Self>, PieFail> {
        Arc::try_new(Self {
            scope,
            control: SpinLock::new((controller, branch)),
        })
        .map_err(|_| PieFail::OoM)
    }

    pub(crate) fn scope(&self) -> ViewControl {
        self.scope
    }

    pub(crate) fn control(&self) -> (TaskId, Option<PieToken>) {
        *self.control.lock()
    }

    pub(crate) fn hand_over(&self, controller: TaskId) {
        *self.control.lock() = (controller, None);
    }

    /// 分支改指：只换历史分支索引，控制身份与权限快照不动。
    pub(crate) fn retarget(&self, forgotten: PieToken, parent: PieToken) {
        let mut control = self.control.lock();
        if control.1 == Some(forgotten) {
            control.1 = Some(parent);
        }
    }
}
