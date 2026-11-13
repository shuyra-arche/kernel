//! Immutable, bounded Memo-export lineage. Only weak resource/Task references
//! are retained: exporting into the same Nest must not create an ownership cycle.
use super::*;
use crate::basis::{
    nest::Span,
    pend::{Export, PendEntry},
};
use kernel_api::model::pend::{Operations, PendScope};

pub(super) struct Lineage {
    grant: Export,
    actor: Weak<Task>,
    task: TaskId,
    token: PendId,
    reservation: Option<Weak<Span>>,
    previous: Option<Arc<Self>>,
    depth: usize,
}

// PendId's !Send marker governs userspace handle ownership. Here it is an
// immutable lookup key paired with the original Task and exact live Pend;
// resource mutation uses their locks/atomics and the Kernel COMMIT boundary.
unsafe impl Send for Lineage {}
unsafe impl Sync for Lineage {}

impl Lineage {
    pub(super) fn extend(
        previous: Option<Arc<Self>>,
        actor: &Arc<Task>,
        entry: &Arc<PendEntry>,
        operations: Operations,
        scope: PendScope,
        reservation: Option<&Arc<Span>>,
    ) -> Result<Arc<Self>, CellFail> {
        let depth = previous.as_ref().map_or(1, |p| p.depth + 1);
        if depth > 64
            || !pend::live(actor)
            || pend::locate(actor, entry.token()).is_none_or(|held| !Arc::ptr_eq(&held, entry))
            || !entry.permits(operations)
            || reservation.is_some_and(|span| !span.is_open())
        {
            return Err(CellFail::Denied);
        }
        let grant = Export::new(entry.authority(), operations, scope).map_err(failure)?;
        Arc::try_new(Self {
            grant,
            actor: Arc::downgrade(actor),
            task: actor.ident.id,
            token: entry.token(),
            reservation: reservation.map(Arc::downgrade),
            previous,
            depth,
        })
        .map_err(|_| CellFail::OutOfMemory)
    }

    pub(super) fn check(&self, access: Modifiable) -> Result<(), CellFail> {
        let mut next = Some(self);
        while let Some(route) = next {
            let actor = route
                .actor
                .upgrade()
                .filter(|actor| pend::live(actor))
                .ok_or(CellFail::Denied)?;
            let entry = pend::locate(&actor, route.token).ok_or(CellFail::Denied)?;
            if !entry.alive()
                || !Arc::ptr_eq(entry.authority(), route.grant.authority())
                || !route.grant.permits(access)
                || route
                    .reservation
                    .as_ref()
                    .is_some_and(|span| span.upgrade().is_none_or(|span| !span.is_open()))
            {
                return Err(CellFail::Denied);
            }
            next = route.previous.as_deref();
        }
        Ok(())
    }

    pub(super) fn any(&self, mut predicate: impl FnMut(&Self) -> bool) -> bool {
        let mut next = Some(self);
        while let Some(route) = next {
            if predicate(route) {
                return true;
            }
            next = route.previous.as_deref();
        }
        false
    }
    pub(super) fn branch(&self, token: PendId) -> bool {
        self.token == token
    }
    pub(super) fn actor(&self, task: TaskId) -> bool {
        self.task == task
    }
    pub(super) fn reservation(&self, span: &Span) -> bool {
        self.reservation
            .as_ref()
            .is_some_and(|known| core::ptr::eq(known.as_ptr(), span))
    }
    pub(super) fn retiring(&self) -> bool {
        !self.grant.authority().alive()
            || self.actor.upgrade().is_none_or(|actor| !pend::live(&actor))
            || self
                .reservation
                .as_ref()
                .is_some_and(|span| span.upgrade().is_none_or(|span| !span.is_open()))
    }
}
