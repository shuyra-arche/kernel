//! An exact publishing route, retained until its hardware use has settled.
use super::super::{Access, Nest, Span};
use crate::basis::pend::{self, Pend, PendEntry};
use crate::basis::unit::task::Task;
use alloc::sync::{Arc, Weak};
use kernel_api::abi::call::cell::CellFail;
use kernel_api::address::{PageCount, PageOffset};
use kernel_api::model::identity::{PendId, TaskId};
use kernel_api::model::modifiable::Modifiable;
use kernel_api::model::pend::{NestRights, NestScope, Operations, PendScope};

/// Identity observations always revalidate against the actual holder ledger.
#[derive(Clone)]
pub(in crate::basis::nest) struct Principal {
    authority: Arc<Pend>,
    actor: Weak<Task>,
    token: PendId,
}

impl Principal {
    pub(in crate::basis::nest) fn authority(&self) -> &Arc<Pend> {
        &self.authority
    }

    pub(in crate::basis::nest) fn actor(&self) -> Option<Arc<Task>> {
        self.actor.upgrade().filter(|actor| pend::live(actor))
    }

    pub(in crate::basis::nest) fn check(
        &self,
        nest: &Nest,
        span: &Span,
        page: usize,
        access: Access,
    ) -> Result<Arc<Task>, CellFail> {
        let actor = self.actor().ok_or(CellFail::Denied)?;
        let entry = pend::locate(&actor, self.token).ok_or(CellFail::Denied)?;
        let count = PageCount::new(span.grain()).ok_or(CellFail::NotAligned)?;
        if !Arc::ptr_eq(entry.authority(), &self.authority)
            || !entry.permits(Operations::Nest(NestRights::PUBLISH))
            || entry
                .target()
                .nest()
                .is_none_or(|owner| !core::ptr::eq(Arc::as_ptr(&owner), nest))
            || !matches!(self.authority.scope(), PendScope::Nest(NestScope::Mapping { id, .. })
                if id.get() == span.identity())
            || !self.authority.contains(PageOffset::new(page), count)
            || access.contains(Access::WRITE) && entry.modifiable() != Modifiable::Yes
        {
            return Err(CellFail::Denied);
        }
        Ok(actor)
    }
}

pub(crate) struct Publisher {
    principal: Option<Principal>,
    task: TaskId,
    published: bool,
}

impl Publisher {
    /// Caller holds the actor's Pend serial and COMMIT.
    pub(crate) fn new(actor: &Arc<Task>, entry: &Arc<PendEntry>) -> Result<Self, CellFail> {
        if !pend::live(actor)
            || !entry.permits(Operations::Nest(NestRights::PUBLISH))
            || !matches!(
                entry.authority().scope(),
                PendScope::Nest(NestScope::Mapping { .. })
            )
            || entry.target().nest().is_none()
            || pend::locate(actor, entry.token()).is_none_or(|held| !Arc::ptr_eq(&held, entry))
        {
            return Err(CellFail::Denied);
        }
        entry
            .authority()
            .retain_route()
            .map_err(super::pend_failure)?;
        Ok(Self {
            principal: Some(Principal {
                authority: entry.authority().clone(),
                actor: Arc::downgrade(actor),
                token: entry.token(),
            }),
            task: actor.ident.id,
            published: false,
        })
    }

    pub(in crate::basis::nest) fn authority(&self) -> &Arc<Pend> {
        &self
            .principal
            .as_ref()
            .expect("retained publishing route")
            .authority
    }

    pub(in crate::basis::nest) fn observe(&self) -> Principal {
        self.principal
            .as_ref()
            .expect("retained publishing route")
            .clone()
    }

    pub(in crate::basis::nest) fn publish(&mut self) {
        self.published = true;
    }

    /// # Safety
    /// No hardware attempt occurred, or exact publication completion was proved.
    pub(in crate::basis::nest) unsafe fn settle(mut self) -> (TaskId, PendId) {
        let principal = self.principal.take().expect("retained publishing route");
        unsafe { principal.authority.release_route() };
        self.published = false;
        (self.task, principal.token)
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        if let Some(principal) = self.principal.take() {
            if self.published {
                // Lost completion responsibility cannot return a live route.
                core::mem::forget(principal);
            } else {
                let _commit = crate::basis::commit();
                // SAFETY: cancellation is possible only before any hardware attempt.
                unsafe { principal.authority.release_route() };
            }
        }
    }
}

// The PendId is a holder observation; it never leaves the kernel as authority.
unsafe impl Send for Publisher {}
