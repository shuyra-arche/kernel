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
mod lineage;
use lineage::Lineage;
pub(crate) mod exports;

#[derive(Clone)]
pub(crate) struct Source {
    hold: Arc<Hold>,
    offset: PageOffset,
    count: PageCount,
    actor: Weak<Task>,
    /// Cleanup identity only; authority always uses the live original actor.
    task: TaskId,
    ceiling: Access,
    /// Descriptive fixed origin survives revocation; never used as authority.
    position: (kernel_api::model::flat::FlatId, usize),
    lineage: Option<Arc<Lineage>>,
}

impl Source {
    /// Select directly from a live Flat/Share Pend, without creating a SlotId.
    /// Offset is relative to the immutable source range, not the narrowed Pend.
    /// Caller holds its ledger serial and COMMIT until association is installed.
    pub(crate) fn from_pend(
        actor: &Arc<Task>,
        token: PendId,
        offset: PageOffset,
        count: PageCount,
    ) -> Result<Self, CellFail> {
        use kernel_api::model::pend::{FlatRights, Operations, PendRange, PendScope, ShareRights};
        if !pend::live(actor) {
            return Err(CellFail::Denied);
        }
        let entry = pend::locate(actor, token).ok_or(CellFail::Denied)?;
        if !entry.associates() {
            return Err(CellFail::Denied);
        }
        let meta = entry.target().memory().ok_or(CellFail::Denied)?;
        let authorization = meta.authorization().ok_or(CellFail::Denied)?;
        if matches!(entry.target(), pend::Target::Flat(_)) {
            let page = meta
                .source()
                .index()
                .get()
                .checked_add(offset.get())
                .ok_or(CellFail::NotAligned)?;
            let position =
                kernel_api::model::flat::SlotPosition::new(token, meta.source().source_id(), page)
                    .ok_or(CellFail::NotAligned)?;
            let selected =
                crate::basis::flat::authority::slot(actor, position, FlatRights::ASSOCIATE)
                    .map_err(failure)?;
            if selected.page_count() != count {
                return Err(CellFail::NotAligned);
            }
        } else if !entry.authority().selects(
            Operations::Share(ShareRights::ASSOCIATE),
            PendScope::Pages(PendRange::Pages {
                offset: offset.get(),
                count: count.get(),
            }),
        ) {
            return Err(CellFail::Denied);
        }
        if authorization
            .full_range()
            .is_none_or(|range| range.range(offset, count).is_err())
        {
            return Err(CellFail::NoRegion);
        }
        authorization.ensure_internal().map_err(failure)?;
        let ceiling = authorization.ceiling();
        let hold = Hold::from_branch(
            authorization,
            offset,
            count,
            ceiling,
            (token, entry.authority().clone()),
        )
        .map_err(failure)?;
        let selected = Self::new(actor, hold, PageOffset::ZERO, count)?;
        // A source may outlive this call through a foreign Nest. Losing the last
        // public Slot must not remove the actor's route to its remaining uses.
        actor
            .pend
            .references
            .track(selected.source())
            .map_err(failure)?;
        Ok(selected)
    }

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
        let selected = full
            .range(PageOffset::new(offset), count)
            .map_err(|_| CellFail::NotAligned)?;
        if count.get() != selected.grain() {
            return Err(CellFail::NotAligned);
        }
        let ceiling = hold.ceiling() & hold.source().ceiling();
        let position = (
            full.source_id(),
            full.index()
                .get()
                .checked_add(offset)
                .ok_or(CellFail::NotAligned)?,
        );
        let mut selected = Self {
            hold,
            offset: PageOffset::new(offset),
            count,
            actor: Arc::downgrade(actor),
            task: actor.ident.id,
            ceiling,
            position,
            lineage: None,
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

    /// Resolve only descriptive holding metadata. Cleanup must not depend on
    /// the data grant that has just been revoked or lent to an External user.
    pub(crate) fn provenance(&self) -> Option<Arc<pend::Provenance>> {
        self.source()
            .full_range()?
            .range(self.offset, self.count)
            .ok()?
            .provenance()
    }
    pub(crate) fn hold_provenance(
        &self,
    ) -> Result<alloc::vec::Vec<crate::basis::flat::sharing::SharingOperation>, CellFail> {
        self.provenance().map_or_else(
            || Ok(alloc::vec::Vec::new()),
            |p| p.operations(self.source()).map_err(failure),
        )
    }
    pub(crate) fn admit_provenance(&self, owner: Option<&Pend>) -> Result<(), CellFail> {
        if let Some(origin) = self.provenance() {
            origin.admit(owner).map_err(failure)?;
        }
        Ok(())
    }
    fn check_provenance(&self, need: Access, export: bool) -> Result<(), CellFail> {
        if let Some(origin) = self.provenance() {
            origin
                .check(
                    self.task,
                    Some(need),
                    kernel_api::model::pend::FlatRights::ASSOCIATE,
                    export || self.lineage.is_some(),
                )
                .map_err(failure)?;
        }
        Ok(())
    }
    pub(crate) fn range(&self) -> Result<FlatRange, CellFail> {
        self.check_provenance(Access::READ, false)?;
        if let Some(lineage) = &self.lineage {
            lineage.check(Modifiable::No)?;
        }
        let actor = self.actor().ok_or(CellFail::Denied)?;
        self.source().ensure_internal().map_err(failure)?;
        let _authority = self.authority(&actor)?;
        let range = self
            .hold
            .resolve_window(self.offset, self.count)
            .ok_or(CellFail::Denied)?;
        if range.page_count().get() != range.grain() {
            return Err(CellFail::NotAligned);
        }
        Ok(range)
    }

    pub(crate) fn check(&self, need: Access) -> Result<FlatRange, CellFail> {
        self.check_provenance(need, false)?;
        if let Some(lineage) = &self.lineage {
            lineage.check(if need.contains(Access::WRITE) {
                Modifiable::Yes
            } else {
                Modifiable::No
            })?;
        }
        if !self.ceiling.contains(need) {
            return Err(CellFail::Denied);
        }
        let actor = self.actor().ok_or(CellFail::Denied)?;
        self.source().ensure_internal().map_err(failure)?;
        let authority = self.authority(&actor)?;
        if !self.source().branch_check(authority.as_deref(), need) {
            return Err(CellFail::Denied);
        }
        let range = self
            .hold
            .resolve_window(self.offset, self.count)
            .ok_or(CellFail::Denied)?;
        if range.page_count().get() != range.grain() {
            return Err(CellFail::NotAligned);
        }
        Ok(range)
    }

    pub(in crate::basis::nest) fn begin_cpu(
        &self,
        id: usize,
        need: Access,
        maintenance: Option<&Pend>,
    ) -> Result<PublicationFlight, CellFail> {
        let root = self.source().root();
        let operation = root
            .as_ref()
            .map(|(meta, _, _)| {
                crate::basis::flat::sharing::SharingOperation::acquire(meta).ok_or(CellFail::Busy)
            })
            .transpose()?;
        let actor = self.actor().ok_or(CellFail::Denied)?;
        self.check(need)?;
        self.admit_provenance(maintenance)?;
        let authority = self.authority(&actor)?;
        self.source()
            .check_move(self.offset, self.count, maintenance)
            .map_err(failure)?;
        self.source()
            .enter_publish(
                id,
                authority.as_deref(),
                need,
                self.offset,
                self.count,
                maintenance,
            )
            .map_err(failure)?;
        Ok(PublicationFlight {
            source: self.source().clone(),
            id,
            _operation: operation,
        })
    }

    pub(crate) fn limited(&self, ceiling: Access) -> Self {
        Self {
            ceiling: self.ceiling & ceiling,
            ..self.clone()
        }
    }

    /// Export only through the actual caller's live Reservation PUBLISH grant.
    /// Source, original and intermediate distribution qualifications remain
    /// live dependencies through the destination Memo's hardware retirement.
    pub(crate) fn export(
        &self,
        actor: &Arc<Task>,
        route: &pend::access::Reservation,
        index: usize,
    ) -> Result<Self, CellFail> {
        use kernel_api::model::pend::{
            FlatRights, NestRights, NestScope, Operations, PendRange, PendScope, ShareRights,
        };
        self.check_provenance(Access::READ, true)?;
        if self.range()?.staged() {
            return Err(CellFail::Denied);
        }
        let span = route.span().map_err(failure)?;
        if span.grain() != self.count.get() {
            return Err(CellFail::NotAligned);
        }
        let (mut ceiling, _) = span.access(index)?;
        if route.entry().modifiable() != Modifiable::Yes {
            ceiling.remove(Access::WRITE);
        }
        let previous = if let Some(lineage) = &self.lineage {
            lineage.clone()
        } else {
            let original = self.actor().ok_or(CellFail::Denied)?;
            let authority = self.authority(&original)?.ok_or(CellFail::Denied)?;
            let (_, root, _) = self.source().root().ok_or(CellFail::Denied)?;
            let token = self.hold.branch().map_or(root, |(token, _)| token);
            let entry = pend::locate(&original, token).ok_or(CellFail::Denied)?;
            if !Arc::ptr_eq(entry.authority(), &authority) {
                return Err(CellFail::Denied);
            }
            let operations = match entry.target() {
                pend::Target::Flat(_) => Operations::Flat(FlatRights::ASSOCIATE),
                pend::Target::View(crate::basis::view::View::Share(_)) => {
                    Operations::Share(ShareRights::ASSOCIATE)
                }
                _ => return Err(CellFail::Denied),
            };
            Lineage::extend(
                None,
                &original,
                &entry,
                operations,
                PendScope::Pages(PendRange::Pages {
                    offset: self.offset.get(),
                    count: self.count.get(),
                }),
                None,
            )?
        };
        let lineage = Lineage::extend(
            Some(previous),
            actor,
            route.entry(),
            Operations::Nest(NestRights::PUBLISH),
            PendScope::Nest(NestScope::Reservation {
                id: route.id(),
                range: PendRange::Pages {
                    offset: index,
                    count: self.count.get(),
                },
            }),
            Some(&span),
        )?;
        actor
            .pend
            .references
            .track(self.source())
            .map_err(failure)?;
        span.exports
            .lock()
            .remember(self.source())
            .map_err(failure)?;
        Ok(Self {
            ceiling: self.ceiling & ceiling,
            lineage: Some(lineage),
            ..self.clone()
        })
    }

    pub(crate) fn ceiling(&self) -> Access {
        self.ceiling
    }
    pub(crate) fn position(&self) -> (kernel_api::model::flat::FlatId, usize) {
        self.position
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
        if self
            .lineage
            .as_ref()
            .is_some_and(|route| route.any(|route| route.branch(token)))
        {
            return true;
        }
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
            || self
                .lineage
                .as_ref()
                .is_some_and(|route| route.any(|route| route.actor(task)))
    }

    pub(crate) fn uses_reservation(&self, span: &crate::basis::nest::Span) -> bool {
        self.lineage
            .as_ref()
            .is_some_and(|route| route.any(|route| route.reservation(span)))
    }

    pub(crate) fn exported_by(&self, task: TaskId, token: PendId) -> bool {
        self.lineage
            .as_ref()
            .is_some_and(|lineage| lineage.any(|route| route.actor(task) && route.branch(token)))
    }

    /// Logical source loss, independent of a legitimate external borrow's suspension.
    pub(crate) fn retiring(&self) -> bool {
        let root = self.source().root();
        self.provenance().is_some_and(|origin| origin.retiring())
            || self.actor().is_none()
            || self
                .lineage
                .as_ref()
                .is_some_and(|route| route.any(Lineage::retiring))
            || self.source().revoked()
            || self
                .hold
                .branch()
                .is_some_and(|(_, authority)| authority.retiring())
            || root
                .as_ref()
                .is_some_and(|(meta, _, authority)| !meta.alive() || authority.retiring())
            || root.is_none() && self.source().own_range().is_none()
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
        maintenance: Option<&Pend>,
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
        self.admit_provenance(maintenance)?;
        ExternalBorrow::prepare(
            &self.hold,
            &actor,
            self.offset,
            self.count,
            modifiable,
            Some(change),
            maintenance,
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

pub(in crate::basis::nest) struct PublicationFlight {
    source: Arc<SourceAuthorization>,
    id: usize,
    _operation: Option<crate::basis::flat::sharing::SharingOperation>,
}

impl Drop for PublicationFlight {
    fn drop(&mut self) {
        let _commit = crate::basis::commit();
        self.source.leave_publish(self.id);
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
