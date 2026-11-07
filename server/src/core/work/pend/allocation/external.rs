//! Kernel-retained device reborrow. A private allocation reserves the whole source;
//! a View uses the same Pend borrow tree as CPU access. Neither returns on timeout.
use super::{hold::Hold, source_authorization::SourceAuthorization};
use crate::core::{
    memory::{
        cell::{Cell, Exposure},
        flat::Pages,
        nest::Access,
    },
    work::{
        pend::{self, authority::Pend},
        unit::task::Task,
        view::share_view::ShareOperation,
    },
};
use alloc::sync::Arc;
use kernel_api::{
    abi::call::pend::PendFail,
    model::{memory::CellOwner, modifiable::Modifiable},
};

pub(crate) struct ExternalBorrow {
    source: Option<Arc<SourceAuthorization>>,
    pages: Option<Pages>,
    cell: Arc<Cell>,
    loan: Option<Arc<Pend>>,
    parent: Option<Arc<Pend>>,
    published: bool,
    observer: Option<(
        kernel_api::model::identity::TaskId,
        kernel_api::model::identity::PendId,
    )>,
}
impl ExternalBorrow {
    /// Caller holds the work commit boundary and the live Task's Pend table lock.
    pub(crate) fn prepare(hold: &Hold, task: &Task, modifiable: bool) -> Result<Self, PendFail> {
        let access = if modifiable {
            Modifiable::Yes
        } else {
            Modifiable::No
        };
        let need = if modifiable {
            Access::READ | Access::WRITE
        } else {
            Access::READ
        };
        if !hold.ceiling().contains(need) || hold.retains() {
            return Err(PendFail::Denied);
        }
        let source = hold.source();
        let range = hold.resolve().ok_or(PendFail::Denied)?;
        if range.page_count().get() != 1 || range.is_borrowed() {
            return Err(PendFail::Denied);
        }
        let pages = range.materialize().map_err(|_| PendFail::OoM)?;
        let cells = range.materialized_cells().map_err(|_| PendFail::OoM)?;
        let [(_, cell)] = cells.as_slice() else {
            return Err(PendFail::Denied);
        };
        let (loan, parent, observer) = if let Some((meta, root, authority)) = source.root() {
            let (token, authority) = hold.branch().unwrap_or((root, &authority));
            let entry = pend::locate(task, token).ok_or(PendFail::Denied)?;
            if !entry.alive() || !Arc::ptr_eq(entry.authority(), authority) || source.revoked() {
                return Err(PendFail::Denied);
            }
            let _operation = ShareOperation::acquire(&meta).ok_or(PendFail::Busy)?;
            let _publication = source.begin_withdraw()?;
            let (offset, count) = hold.window();
            source.check_publish(offset, count, None)?;
            // A writable parent must explicitly unmap before external publication.
            // Immutable parents and their read-only CPU aliases may coexist.
            if authority.configured() == Modifiable::Yes && meta.pending_maps(token) {
                return Err(PendFail::Busy);
            }
            let loan = Pend::try_new_window(access, Some(hold.window()))?;
            authority.borrow_external(&loan)?;
            (
                Some(loan),
                Some(authority.clone()),
                Some((task.ident.id, token)),
            )
        } else {
            if cell.owner() != CellOwner::Team(task.ident.team.id)
                || !source
                    .own_range()
                    .is_some_and(|full| full.same_range(&range))
            {
                return Err(PendFail::Denied);
            }
            source.begin_external(task.ident.id)?;
            (None, None, None)
        };
        Ok(Self {
            source: Some(source.clone()),
            pages: Some(pages),
            cell: cell.clone(),
            loan,
            parent,
            published: false,
            observer,
        })
    }
    pub(crate) fn cell(&self) -> &Arc<Cell> {
        &self.cell
    }
    pub(crate) fn publish(&mut self) {
        self.published = true;
    }
    pub(crate) fn revoked(&self) -> bool {
        self.parent.as_ref().is_some_and(|parent| parent.retiring())
            || self.source.as_ref().is_some_and(|source| {
                source.revoked()
                    || self.parent.is_some()
                        && source.root().is_none_or(|(meta, _, _)| !meta.alive())
            })
    }
    /// # Safety
    /// Matching IoSpace completion has settled this exact device relationship;
    /// caller holds work commit. Other read-only device relationships may remain.
    pub(crate) unsafe fn settle(
        mut self,
    ) -> Option<(
        kernel_api::model::identity::TaskId,
        kernel_api::model::identity::PendId,
    )> {
        if self.loan.is_none() {
            assert_eq!(self.cell.exposure(), Exposure::Internal);
        }
        self.published = false;
        self.observer
    }
}
impl Drop for ExternalBorrow {
    fn drop(&mut self) {
        if self.published {
            core::mem::forget(self.pages.take());
            core::mem::forget(self.source.take());
            core::mem::forget(self.loan.take());
            core::mem::forget(self.parent.take());
        } else if let Some(loan) = self.loan.take() {
            loan.settle().expect("settled device leaf has no children");
        } else if let Some(source) = self.source.take() {
            source.end_external();
        }
    }
}
