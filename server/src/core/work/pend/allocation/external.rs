//! Exclusive external access to one private managed page. CPU aliases must be
//! explicitly detached and settled first; an Arc or an empty PTE is not proof.
use super::{hold::Hold, source_authorization::SourceAuthorization};
use crate::core::memory::{cell::Cell, flat::Pages, nest::Access};
use alloc::sync::Arc;
use kernel_api::{abi::call::pie::PieFail, model::identity::TaskId};

pub(crate) struct ExternalBorrow {
    source: Option<Arc<SourceAuthorization>>,
    pages: Option<Pages>,
    cell: Arc<Cell>,
    published: bool,
}
impl ExternalBorrow {
    /// Must run under the work commit boundary with the live Task's Pend locked.
    pub(crate) fn prepare(hold: &Hold, task: TaskId, modifiable: bool) -> Result<Self, PieFail> {
        let need = if modifiable {
            Access::READ | Access::WRITE
        } else {
            Access::READ
        };
        if !hold.ceiling().contains(need) || hold.retains() || hold.grant().is_some() {
            return Err(PieFail::Denied);
        }
        let source = hold.source();
        let range = hold.resolve().ok_or(PieFail::Denied)?;
        if range.page_count().get() != 1
            || range.is_borrowed()
            || !source
                .own_range()
                .is_some_and(|full| full.same_range(&range))
        {
            return Err(PieFail::Denied);
        }
        source.begin_external(task)?;
        let result = (|| {
            let pages = range.materialize().map_err(|_| PieFail::OoM)?;
            let cells = range.materialized_cells().map_err(|_| PieFail::OoM)?;
            let [(_, cell)] = cells.as_slice() else {
                return Err(PieFail::Denied);
            };
            Ok(Self {
                source: Some(source.clone()),
                pages: Some(pages),
                cell: cell.clone(),
                published: false,
            })
        })();
        if result.is_err() {
            source.end_external();
        }
        result
    }
    pub(crate) fn cell(&self) -> &Arc<Cell> {
        &self.cell
    }
    pub(crate) fn publish(&mut self) {
        self.published = true;
    }
    /// # Safety
    /// The matching IoSpace binding has settled all hardware responsibilities.
    /// The caller holds the work commit boundary while reopening source access.
    pub(crate) unsafe fn settle(mut self) {
        self.published = false;
    }
}
impl Drop for ExternalBorrow {
    fn drop(&mut self) {
        if self.published {
            // Losing the coordinator cannot reopen CPU access or migration while
            // DMA may still exist. Normal failure keeps the whole record instead.
            core::mem::forget(self.pages.take());
            core::mem::forget(self.source.take());
        } else if let Some(source) = self.source.take() {
            source.end_external();
        }
    }
}
