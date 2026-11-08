use crate::basis::cell::page::{PageId, Retention};

use super::{BindingId, Phase};

pub(super) struct Binding {
    pub(super) projection: Projection,
    pub(super) retention: Option<Retention>,
    pub(super) phase: Phase,
}

impl Binding {
    pub(super) fn begin_retirement(&mut self) {
        let retained = self.retention.take().expect("one retention per binding");
        self.retention = Some(retained.retire());
    }
}

/// 一整个托管 Page 的设备地址投影；身份不从 IOVA 或 PA 推导。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Projection {
    pub(crate) id: BindingId,
    pub(crate) cell: PageId,
    pub(crate) iova: usize,
    pub(crate) physical: usize,
    pub(crate) size: usize,
    pub(crate) modifiable: bool,
}
