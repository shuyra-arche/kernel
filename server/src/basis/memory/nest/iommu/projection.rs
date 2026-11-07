use crate::basis::memory::cell::CellId;

use super::BindingId;

/// 一整个托管 Cell 的设备地址投影；身份不从 IOVA 或 PA 推导。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Projection {
    pub(crate) id: BindingId,
    pub(crate) cell: CellId,
    pub(crate) iova: usize,
    pub(crate) physical: usize,
    pub(crate) size: usize,
    pub(crate) modifiable: bool,
}
