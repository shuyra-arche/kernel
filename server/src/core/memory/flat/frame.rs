use alloc::sync::Arc;

use crate::core::memory::cell::{Cell, CellId};

use super::custody::Custody;
use super::error::FlatError;

#[derive(Debug)]
pub struct Frame {
    /// Flat 的单页承载适配；物理身份与回收凭证只保存在 Cell 中。
    cell: Cell,
    custody: Custody,
}

impl Frame {
    pub fn allocate() -> Result<Arc<Self>, FlatError> {
        let cell = super::allocator::frame::cell()?;
        Arc::try_new(Self {
            cell,
            custody: Custody::new(),
        })
        .map_err(|_| FlatError::OutOfMemory)
    }

    /// 这份帧的迁移托管：只有 `Sole` 能开始移交。
    pub(super) fn custody(&self) -> &Custody {
        &self.custody
    }

    pub fn physical_address(&self) -> usize {
        self.cell.physical_address()
    }

    pub(crate) fn cell_id(&self) -> CellId {
        self.cell.id()
    }

    #[cfg(debug_assertions)]
    pub fn as_ptr(&self) -> *const u8 {
        self.cell.as_mut_ptr().cast_const()
    }

    pub fn as_mut_ptr(&self) -> *mut u8 {
        self.cell.as_mut_ptr()
    }
}
