//! One selected source Cell, its original authority and a sticky access ceiling.
use crate::basis::cell::page::{Change, Page};
use crate::basis::flat::FlatRange;
use crate::basis::nest::Access;
use crate::basis::pend::{self, ExternalBorrow, Hold, Pend, SourceAuthorization};
use crate::basis::unit::task::Task;
use alloc::sync::{Arc, Weak};
use kernel_api::abi::call::{cell::CellFail, pend::PendFail};
use kernel_api::address::{PAGE_SIZE, PageCount, PageOffset};
use kernel_api::model::identity::{PendId, TaskId};
use kernel_api::model::modifiable::Modifiable;

#[derive(Clone)]
pub(crate) struct Source {
    hold: Arc<Hold>,
    offset: PageOffset,
    count: PageCount,
    actor: Weak<Task>,
    /// Cleanup identity only; authority always uses the live original actor.
    task: TaskId,
    ceiling: Access,
}

impl Source {
    pub(crate) fn new(
        actor: &Arc<Task>,
        hold: Arc<Hold>,
        page: PageOffset,
        count: PageCount,
    ) -> Result<Self, CellFail> {
        if !pend::live(actor) {
            return Err(CellFail::Denied);
        }
        let (base, size) = hold.window();
        let relative_end = page
            .get()
            .checked_add(count.get())
            .ok_or(CellFail::NotAligned)?;
        if relative_end > size.get() {
            return Err(CellFail::NoRegion);
        }
        let offset = base
            .get()
            .checked_add(page.get())
            .ok_or(CellFail::NotAligned)?;
        let end = offset
            .checked_add(count.get())
            .ok_or(CellFail::NotAligned)?;
        base.get()
            .checked_add(size.get())
            .ok_or(CellFail::NotAligned)?;
        end.checked_mul(PAGE_SIZE).ok_or(CellFail::NotAligned)?;
        let full = hold.source().full_range().ok_or(CellFail::Denied)?;
        if count.get() != full.grain()
            || !page.get().is_multiple_of(full.grain())
            || !offset.is_multiple_of(full.grain())
        {
            return Err(CellFail::NotAligned);
        }
        let ceiling = hold.ceiling() & hold.source().ceiling();
        let mut selected = Self {
            hold,
            offset: PageOffset::new(offset),
            count,
            actor: Arc::downgrade(actor),
            task: actor.ident.id,
            ceiling,
        };
        selected.range()?;
        let authority = selected.authority(actor)?;
        let modifiable = match authority {
            Some(authority) => authority.modifiable(),
            None => selected.source().modifiable(),
        }
        .ok_or(CellFail::Denied)?;
        if modifiable == Modifiable::No {
            selected.ceiling.remove(Access::WRITE);
        }
        Ok(selected)
    }

    pub(crate) fn range(&self) -> Result<FlatRange, CellFail> {
        let actor = self.actor().ok_or(CellFail::Denied)?;
        self.source().ensure_internal().map_err(failure)?;
        let _authority = self.authority(&actor)?;
        self.hold
            .resolve_window(self.offset, self.count)
            .ok_or(CellFail::Denied)
    }

    pub(crate) fn check(&self, need: Access) -> Result<FlatRange, CellFail> {
        if !self.ceiling.contains(need) {
            return Err(CellFail::Denied);
        }
        let actor = self.actor().ok_or(CellFail::Denied)?;
        self.source().ensure_internal().map_err(failure)?;
        let authority = self.authority(&actor)?;
        if !self.source().branch_check(authority.as_deref(), need) {
            return Err(CellFail::Denied);
        }
        self.hold
            .resolve_window(self.offset, self.count)
            .ok_or(CellFail::Denied)
    }

    pub(crate) fn limited(&self, ceiling: Access) -> Self {
        Self {
            ceiling: self.ceiling & ceiling,
            ..self.clone()
        }
    }

    pub(crate) fn ceiling(&self) -> Access {
        self.ceiling
    }

    pub(crate) fn source(&self) -> &Arc<SourceAuthorization> {
        self.hold.source()
    }

    pub(crate) fn hold(&self) -> &Arc<Hold> {
        &self.hold
    }

    pub(crate) fn window(&self) -> (PageOffset, PageCount) {
        (self.offset, self.count)
    }

    pub(crate) fn actor(&self) -> Option<Arc<Task>> {
        self.actor.upgrade().filter(|actor| pend::live(actor))
    }

    pub(crate) fn matches_branch(&self, token: PendId) -> bool {
        match self.hold.branch() {
            Some((branch, _)) => branch == token,
            None => self
                .source()
                .root()
                .is_some_and(|(_, root, _)| root == token),
        }
    }

    pub(crate) fn belongs_to(&self, task: TaskId) -> bool {
        self.task == task
    }

    pub(crate) fn same_page(&self, page: &Arc<Page>) -> Result<FlatRange, CellFail> {
        let range = self.range()?;
        let cells = range.materialized_cells()?;
        if cells.is_empty() {
            return Err(CellFail::SourceEmpty);
        }
        let [(offset, current)] = cells.as_slice() else {
            return Err(CellFail::Denied);
        };
        if *offset != PageOffset::ZERO || !Arc::ptr_eq(current, page) {
            return Err(CellFail::Denied);
        }
        Ok(range)
    }

    /// Caller holds the publisher's Pend serial and COMMIT, but no Nest/source lock.
    pub(crate) fn borrow(
        &self,
        publisher: &Arc<Task>,
        page: &Arc<Page>,
        modifiable: bool,
        change: &Change,
    ) -> Result<ExternalBorrow, CellFail> {
        let actor = self.actor().ok_or(CellFail::Denied)?;
        let _serial = if Arc::ptr_eq(&actor, publisher) {
            None
        } else {
            Some(actor.pend.try_lock().ok_or(CellFail::Busy)?)
        };
        if !pend::live(publisher) || !Arc::ptr_eq(change.page(), page) {
            return Err(CellFail::Denied);
        }
        let need = if modifiable {
            Access::READ | Access::WRITE
        } else {
            Access::READ
        };
        self.check(need)?;
        self.same_page(page)?;
        ExternalBorrow::prepare(
            &self.hold,
            &actor,
            self.offset,
            self.count,
            modifiable,
            Some(change),
        )
        .map_err(failure)
    }

    fn authority(&self, actor: &Task) -> Result<Option<Arc<Pend>>, CellFail> {
        let Some((meta, root, authority)) = self.source().root() else {
            return if self.hold.branch().is_none() && self.source().private_owner(actor.ident.id) {
                Ok(None)
            } else {
                Err(CellFail::Denied)
            };
        };
        let (token, authority) = self.hold.branch().unwrap_or((root, &authority));
        let entry = pend::locate(actor, token).ok_or(CellFail::Denied)?;
        if !entry.associates()
            || !Arc::ptr_eq(entry.authority(), authority)
            || entry
                .target()
                .memory()
                .is_none_or(|target| !Arc::ptr_eq(&target, &meta))
            || meta
                .authority(token)
                .is_none_or(|registered| !Arc::ptr_eq(&registered, authority))
        {
            return Err(CellFail::Denied);
        }
        Ok(Some(authority.clone()))
    }
}

fn failure(error: PendFail) -> CellFail {
    match error {
        PendFail::OutOfMemory => CellFail::OutOfMemory,
        PendFail::Busy => CellFail::Busy,
        PendFail::NotAligned => CellFail::NotAligned,
        _ => CellFail::Denied,
    }
}
