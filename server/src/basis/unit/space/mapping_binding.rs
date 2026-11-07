//! 目标空间与来源记录共享的 CPU 关系身份；不持帧，也不授予视图控制权。

use alloc::sync::Arc;
use alloc::sync::Weak;

use kernel_api::abi::call::pend::PendFail;

use super::Space;

pub(crate) struct MappingBinding {
    id: usize,
    space: Weak<Space>,
}

impl MappingBinding {
    pub(crate) fn new(id: usize, space: &Arc<Space>) -> Result<Arc<Self>, PendFail> {
        Arc::try_new(Self {
            id,
            space: Arc::downgrade(space),
        })
        .map_err(|_| PendFail::OoM)
    }

    pub(crate) fn id(&self) -> usize {
        self.id
    }

    pub(crate) fn space(&self) -> &Weak<Space> {
        &self.space
    }
}
