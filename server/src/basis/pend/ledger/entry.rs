use crate::basis::pend::Pend;

use alloc::sync::Arc;
use alloc::sync::Weak;

use kernel_api::model::identity::Mark;
use kernel_api::model::identity::PendId;
use kernel_api::model::identity::TaskId;

use super::table::{HeirRegistration, Registration};
use crate::basis::pend::Target;
use crate::basis::unit::task::Task;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::address::PageCount;
use kernel_api::model::nest::ReservationId;
use kernel_api::model::pend::{
    Limits, NestRights, NestScope, Operations, PendRange, PendScope, PendStatus,
};

use crate::basis::pend::{Modifiable, Need};

pub(crate) struct PendEntry {
    released: core::sync::atomic::AtomicBool,
    token: PendId,
    authority: Arc<Pend>,
    mark: Mark,
    sire: Option<PendId>,
    lord: Weak<Task>,
    meta: Target,
}

impl PendEntry {
    /// 这一枚 PendEntry 的 Pend：持有它的调用方据此知道自己的分支在哪条撤销边上。
    pub(crate) fn authority(&self) -> &Arc<Pend> {
        &self.authority
    }

    pub(crate) fn token(&self) -> PendId {
        self.token
    }

    pub(crate) fn modifiable(&self) -> Modifiable {
        self.authority.configured()
    }

    pub(crate) fn sire(&self) -> Option<PendId> {
        self.sire
    }

    pub(crate) fn lord(&self) -> &Weak<Task> {
        &self.lord
    }

    pub(crate) fn mark(&self) -> Mark {
        self.mark
    }

    pub(crate) fn scope(&self) -> PendScope {
        self.authority.scope()
    }

    pub(crate) fn alive(&self) -> bool {
        self.meta.alive()
            && self.authority.alive()
            && match self.scope() {
                PendScope::Nest(NestScope::Reservation { id, .. }) => self
                    .meta
                    .nest()
                    .is_some_and(|nest| nest.reservation_open(id)),
                _ => true,
            }
    }

    pub(crate) fn status(&self) -> PendStatus {
        let status = self.authority.status();
        if let Target::Nest { nest, .. } = &self.meta {
            if let PendScope::Nest(NestScope::Reservation { id, .. }) = self.scope() {
                match nest.reservation_status(id) {
                    PendStatus::Dead => return PendStatus::Dead,
                    PendStatus::Retiring if status != PendStatus::Dead => {
                        return PendStatus::Retiring;
                    }
                    _ => {}
                }
            }
            if nest.is_closed() {
                PendStatus::Dead
            } else if !nest.is_open() && status != PendStatus::Dead {
                PendStatus::Retiring
            } else {
                status
            }
        } else if self.meta.alive() {
            status
        } else {
            PendStatus::Dead
        }
    }

    pub(crate) fn allows(&self, need: Need) -> bool {
        self.alive()
            && self
                .authority
                .modifiable()
                .is_some_and(|access| need.allowed_by(access))
    }

    pub(crate) fn permits(&self, operations: Operations) -> bool {
        self.alive() && self.authority.permits(operations)
    }

    pub(crate) fn target(&self) -> &Target {
        &self.meta
    }

    pub(crate) fn derives(&self, limits: Limits, share: bool) -> bool {
        use kernel_api::model::pend::{Distributable, FlatRights, ShareRights};
        let parent = self.authority.limits();
        if share && matches!(self.meta, Target::Flat(_)) {
            let Operations::Flat(rights) = parent.operations else {
                return false;
            };
            let Operations::Share(requested) = limits.operations else {
                return false;
            };
            let mut permitted = ShareRights::empty();
            if rights.contains(FlatRights::ASSOCIATE) {
                permitted |= ShareRights::OPEN | ShareRights::SHUT | ShareRights::ASSOCIATE;
            }
            if rights.contains(FlatRights::DELETE) {
                permitted |= ShareRights::DELETE;
            }
            parent.valid()
                && limits.valid()
                && matches!(
                    (parent.scope, limits.scope),
                    (PendScope::Pages(_), PendScope::Pages(_))
                )
                && parent.distributable == Distributable::Yes
                && parent.modifiable.allows(limits.modifiable)
                && rights.contains(FlatRights::SHARE)
                && permitted.contains(requested)
        } else {
            parent.permits_child(limits)
        }
    }

    pub(crate) fn vest_scope(
        &self,
        mark: Mark,
        limits: Limits,
        share: bool,
    ) -> Result<PendEntry, PendFail> {
        if !self.alive() {
            return Err(PendFail::Dead);
        }
        if !self.derives(limits, share) {
            return Err(PendFail::Denied);
        }
        if let Some(range) = limits.scope.range() {
            let PendRange::Pages { offset, count } = range else {
                return Err(PendFail::Denied);
            };
            if !self.authority.contains(
                kernel_api::address::PageOffset::new(offset),
                PageCount::new(count).ok_or(PendFail::Denied)?,
            ) {
                return Err(PendFail::Denied);
            }
        }
        let target = if share && matches!(self.meta, Target::Flat(_)) {
            Target::View(crate::basis::view::View::Share(
                self.meta
                    .memory()
                    .ok_or(kernel_api::abi::call::pend::PendFail::Denied)?,
            ))
        } else {
            self.meta.clone()
        };
        let entry = PendEntry::scoped(
            target,
            mark,
            limits.modifiable,
            Some(self.token),
            Some(limits.scope),
        )?;
        entry.authority.narrow(limits)?;
        entry.authority.inherit(&self.authority)?;
        Ok(entry)
    }
}

impl PendEntry {
    pub(crate) fn try_new(
        meta: impl Into<Target>,
        mark: Mark,
        modifiable: Modifiable,
        sire: Option<PendId>,
    ) -> Result<PendEntry, kernel_api::abi::call::pend::PendFail> {
        Self::scoped(meta, mark, modifiable, sire, None)
    }

    fn scoped(
        meta: impl Into<Target>,
        mark: Mark,
        modifiable: Modifiable,
        sire: Option<PendId>,
        scope: Option<PendScope>,
    ) -> Result<PendEntry, PendFail> {
        let meta = meta.into();
        let token = next_id()?;
        Ok(PendEntry {
            released: core::sync::atomic::AtomicBool::new(false),
            token,
            authority: meta.new_pend(modifiable, token, scope)?,
            mark,
            sire,
            lord: Weak::new(),
            meta,
        })
    }

    pub(super) fn release(&self) {
        if !self
            .released
            .swap(true, core::sync::atomic::Ordering::AcqRel)
        {
            self.meta.release_pend(self.token);
        }
    }

    pub(crate) fn invalidate(&self) {
        self.authority.invalidate();
        self.meta.changed(self.token, false);
    }

    pub(crate) fn parent(&mut self, token: PendId, task: Weak<Task>) {
        self.sire = Some(token);
        self.lord = task;
    }

    pub(crate) fn narrow(&self, limits: Limits) -> Result<(), PendFail> {
        if !self.alive() {
            return Err(PendFail::Dead);
        }
        self.authority.narrow(limits)?;
        self.meta.changed(
            self.token,
            limits
                .operations
                .contains(kernel_api::model::pend::Operations::Sign(
                    kernel_api::model::sign::SignRights::ROUTE,
                )),
        );
        Ok(())
    }
}

/// Caller holds the Task's Pend serial through preparation and final COMMIT.
pub(crate) fn prepare_reservation<'a>(
    task: &'a Arc<Task>,
    parent: &Arc<PendEntry>,
    id: ReservationId,
    count: PageCount,
) -> Result<PreparedReservationGrant<'a>, PendFail> {
    if !super::live(task) || !parent.alive() {
        return Err(PendFail::Dead);
    }
    let current = locate(task, parent.token()).ok_or(PendFail::Denied)?;
    if !Arc::ptr_eq(&current, parent)
        || parent.scope() != PendScope::Nest(NestScope::Whole)
        || !parent.permits(Operations::Nest(NestRights::RESERVE))
    {
        return Err(PendFail::Denied);
    }
    let ceiling = parent.authority().limits();
    let scope = PendScope::Nest(NestScope::Reservation {
        id,
        range: PendRange::Pages {
            offset: 0,
            count: count.get(),
        },
    });
    let limits = Limits {
        operations: Operations::Nest(NestRights::reservation()),
        modifiable: ceiling.modifiable,
        distributable: ceiling.distributable,
        scope,
    };
    let mut entry = PendEntry::scoped(
        parent.target().clone(),
        Mark::NONE,
        limits.modifiable,
        Some(parent.token()),
        Some(scope),
    )?;
    entry.authority.narrow(limits)?;
    entry.authority.inherit(parent.authority())?;
    entry.parent(parent.token(), Arc::downgrade(task));
    let entry = Arc::try_new(entry).map_err(|_| PendFail::OutOfMemory)?;
    let heir = task
        .pend
        .prepare_heir(parent.token(), Arc::downgrade(task), entry.token())?;
    let registration = task.pend.prepare(entry.clone())?;
    Ok(PreparedReservationGrant {
        entry,
        _parent: parent.clone(),
        heir,
        registration,
    })
}

#[must_use = "an unpublished Reservation rolls back its ledger and heir reservations"]
pub(crate) struct PreparedReservationGrant<'a> {
    entry: Arc<PendEntry>,
    _parent: Arc<PendEntry>,
    heir: HeirRegistration<'a>,
    registration: Registration<'a>,
}

impl PreparedReservationGrant<'_> {
    pub(crate) fn entry(&self) -> &Arc<PendEntry> {
        &self.entry
    }

    /// The caller has committed the Namespace record under the same COMMIT guard.
    pub(crate) fn publish(self) {
        self.heir.publish();
        self.registration.publish();
    }
}

impl Drop for PendEntry {
    fn drop(&mut self) {
        self.authority.invalidate();
        self.release();
    }
}

// SAFETY: identity fields are immutable once shared; authority and View state
// use their own locks/atomics. PendId's !Send marker protects API ownership, not
// this kernel registration, which is always accessed under ledger/COMMIT rules.
unsafe impl Send for PendEntry {}
unsafe impl Sync for PendEntry {}
fn next_id() -> Result<PendId, kernel_api::abi::call::pend::PendFail> {
    static NEXT: crate::basis::name::Serial = crate::basis::name::Serial::new(1, usize::MAX);
    NEXT.allocate()
        .map(PendId::mint)
        .ok_or(kernel_api::abi::call::pend::PendFail::OutOfMemory)
}

pub(crate) fn locate(task: &Task, token: PendId) -> Option<Arc<PendEntry>> {
    let entries = task.pend.entries.lock();
    entries.get(token).cloned()
}

pub(crate) fn vestor(task: &Arc<Task>, token: PendId) -> Option<TaskId> {
    for _ in 0..super::RETRIES {
        let observed = {
            let _pend = task.pend.lock();
            locate(task, token)?
        };
        let parent = observed.sire()?;
        let lord = observed.lord().upgrade()?;
        let result = super::with_pair(task, &lord, || {
            let Some(current) = locate(task, token) else {
                return Some(None);
            };
            if current.sire() != Some(parent) || !current.lord().ptr_eq(observed.lord()) {
                return None;
            }
            Some(locate(&lord, parent).map(|_| lord.ident.id))
        });
        if let Ok(Some(result)) = result {
            return result;
        }
    }
    None
}

pub(crate) fn accede(
    task: &Arc<Task>,
    token: PendId,
    need: Need,
) -> Result<Arc<PendEntry>, kernel_api::abi::call::pend::PendFail> {
    let pend_entry = locate(task, token).ok_or(kernel_api::abi::call::pend::PendFail::Denied)?;
    if !pend_entry.alive() {
        return Err(kernel_api::abi::call::pend::PendFail::Dead);
    }
    if !pend_entry.allows(need) {
        return Err(kernel_api::abi::call::pend::PendFail::Denied);
    }
    Ok(pend_entry)
}

impl PendEntry {
    pub(crate) fn associates(&self) -> bool {
        self.alive() && self.authority.associates()
    }
}
