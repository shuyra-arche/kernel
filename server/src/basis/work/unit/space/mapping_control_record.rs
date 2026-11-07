use alloc::sync::Arc;

use kernel_api::{
    abi::call::pend::PendFail, model::identity::PendId, model::identity::TaskId,
    model::memory::MappingControl,
};

use crate::machine::sync::SpinLock;

pub(crate) struct MappingControlRecord {
    scope: MappingControl,
    control: SpinLock<(TaskId, Option<PendId>)>,
}

impl MappingControlRecord {
    pub(crate) fn new(
        controller: TaskId,
        scope: MappingControl,
        branch: Option<PendId>,
    ) -> Result<Arc<Self>, PendFail> {
        Arc::try_new(Self {
            scope,
            control: SpinLock::new((controller, branch)),
        })
        .map_err(|_| PendFail::OoM)
    }

    pub(crate) fn scope(&self) -> MappingControl {
        self.scope
    }

    pub(crate) fn control(&self) -> (TaskId, Option<PendId>) {
        *self.control.lock()
    }
}
