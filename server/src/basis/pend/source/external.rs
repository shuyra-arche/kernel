//! Kernel-retained device reborrow. A private allocation reserves the whole source;
//! a View uses the same Pend borrow tree as CPU access. Neither returns on timeout.
use super::SourceAuthorization;
use super::hold::Hold;
use crate::basis::cell::page::Exposure;
use crate::basis::cell::page::Page;
use crate::basis::flat::sharing::SharingOperation;
use crate::basis::flat::{FlatError, Pages};
use crate::basis::nest::Access;
use crate::basis::pend;
use crate::basis::pend::Pend;
use crate::basis::unit::task::Task;
use alloc::sync::Arc;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::address::{PageCount, PageOffset};
use kernel_api::model::cell::PageOwner;
use kernel_api::model::modifiable::Modifiable;
use kernel_api::model::pend::{CreationRights, Distributable, Operations, PendRange, PendScope};

pub(crate) struct ExternalBorrow {
    source: Option<Arc<SourceAuthorization>>,
    pages: Option<Pages>,
    cell: Arc<Page>,
    loan: Option<Arc<Pend>>,
    parent: Option<Arc<Pend>>,
    published: bool,
    observer: Option<(
        kernel_api::model::identity::TaskId,
        kernel_api::model::identity::PendId,
    )>,
}
// The borrow owns its resources and moves between cleanup harts under COMMIT.
// The observer PendId is notification metadata, not transferable user authority.
unsafe impl Send for ExternalBorrow {}

impl ExternalBorrow {
    /// Caller holds COMMIT and the source Task's Pend serial, but no Nest/source lock.
    pub(crate) fn prepare(
        hold: &Hold,
        task: &Task,
        offset: PageOffset,
        count: PageCount,
        modifiable: bool,
        change: Option<&crate::basis::cell::page::Change>,
    ) -> Result<Self, PendFail> {
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
        let source = hold.source();
        if !pend::live(task) || !hold.ceiling().contains(need) || !source.ceiling().contains(need) {
            return Err(PendFail::Denied);
        }
        source.ensure_internal()?;
        let range = hold.resolve_window(offset, count).ok_or(PendFail::Denied)?;
        if range.page_count().get() != range.grain()
            || range.is_borrowed()
            || !source.branch_check(hold.authority().map(Arc::as_ref), need)
        {
            return Err(PendFail::Denied);
        }
        let root = source.root();
        if let Some((meta, root, authority)) = &root {
            let (token, authority) = hold.branch().unwrap_or((*root, authority));
            let entry = pend::locate(task, token).ok_or(PendFail::Denied)?;
            if !entry.associates()
                || !Arc::ptr_eq(entry.authority(), authority)
                || !authority.contains(offset, count)
                || entry
                    .target()
                    .memory()
                    .is_none_or(|target| !Arc::ptr_eq(&target, meta))
                || meta
                    .authority(token)
                    .is_none_or(|registered| !Arc::ptr_eq(&registered, authority))
            {
                return Err(PendFail::Denied);
            }
        } else if hold.branch().is_some() || !source.private_owner(task.ident.id) {
            return Err(PendFail::Denied);
        }
        let pages = if let Some(change) = change {
            Pages::new(range.reside_during(change).map_err(failure)?)
        } else {
            range.materialize().map_err(failure)?
        };
        let cells = range.materialized_cells().map_err(failure)?;
        let [(at, cell)] = cells.as_slice() else {
            return Err(PendFail::Denied);
        };
        if *at != PageOffset::ZERO || change.is_some_and(|change| !Arc::ptr_eq(change.page(), cell))
        {
            return Err(PendFail::Denied);
        }
        let (loan, parent, observer) = if let Some((meta, root, authority)) = root {
            let (token, authority) = hold.branch().unwrap_or((root, &authority));
            let _operation = SharingOperation::acquire(&meta).ok_or(PendFail::Busy)?;
            let withdrawal = source.begin_withdraw()?;
            withdrawal.check_publish(offset, count)?;
            // A writable parent must explicitly unmap before external publication.
            // Immutable parents and their read-only CPU aliases may coexist.
            if authority.configured() == Modifiable::Yes && meta.pending_maps(token) {
                return Err(PendFail::Busy);
            }
            let loan = Pend::try_new_scope(
                access,
                PendScope::Pages(PendRange::Pages {
                    offset: offset.get(),
                    count: count.get(),
                }),
                Operations::None(CreationRights::empty()),
                Distributable::No,
            )?;
            authority.borrow_external(&loan)?;
            (
                Some(loan),
                Some(authority.clone()),
                Some((task.ident.id, token)),
            )
        } else {
            if cell.owner() != PageOwner::Team(task.ident.team.id)
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
    pub(crate) fn cell(&self) -> &Arc<Page> {
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
    /// Matching Nest completion has settled this exact device relationship;
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
        if let Some(loan) = self.loan.take() {
            loan.invalidate();
            loan.settle().expect("settled device leaf has no children");
            // Root loans never acquired the private-source external flag.
            self.source = None;
        }
        self.published = false;
        self.observer
    }
}
fn failure(error: FlatError) -> PendFail {
    match error {
        FlatError::OutOfMemory => PendFail::OutOfMemory,
        FlatError::Busy => PendFail::Busy,
        FlatError::NotAligned => PendFail::NotAligned,
        FlatError::OutOfRange | FlatError::Missing => PendFail::Denied,
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
            // SAFETY: a published borrow takes the fail-closed branch above;
            // trusted settlement already consumed its loan in settle().
            unsafe { loan.cancel_unpublished() }.expect("unpublished device leaf has no children");
        } else if let Some(source) = self.source.take() {
            source.end_external();
        }
    }
}
