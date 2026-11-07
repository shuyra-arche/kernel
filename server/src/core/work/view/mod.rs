pub mod clone_view;
pub mod share_view;

use alloc::sync::{Arc, Weak};
use core::sync::atomic::{AtomicUsize, Ordering};

use kernel_api::{
    abi::call::pend::PendFail, model::identity::PendId, model::identity::TaskId,
    model::modifiable::Modifiable, model::view::ViewKind,
};

use crate::core::work::pend::authority::Pend;
use crate::core::work::unit::life::Life;

pub(crate) use clone_view::CloneView;
pub(crate) use share_view::ShareView;

pub(crate) fn next_id() -> Result<usize, PendFail> {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
        id.checked_add(1)
            .filter(|next| *next <= isize::MAX as usize)
    })
    .map_err(|_| PendFail::OoM)
}

#[derive(Clone)]
pub(crate) enum View {
    Clone(Arc<CloneView>),
    Share(Arc<ShareView>),
}

impl View {
    pub(crate) fn kind(&self) -> ViewKind {
        match self {
            View::Clone(_) => ViewKind::Clone,
            View::Share(_) => ViewKind::Share,
        }
    }

    pub(crate) fn owner(&self) -> TaskId {
        match self {
            View::Clone(meta) => meta.owner(),
            View::Share(meta) => meta.owner(),
        }
    }

    pub(crate) fn id(&self) -> usize {
        match self {
            View::Clone(meta) => meta.id,
            View::Share(meta) => meta.id,
        }
    }

    pub(crate) fn life(&self) -> Weak<Life> {
        match self {
            View::Clone(meta) => meta.life(),
            View::Share(meta) => meta.life(),
        }
    }

    pub(crate) fn alive(&self) -> bool {
        match self {
            View::Clone(meta) => meta.alive(),
            View::Share(meta) => meta.alive(),
        }
    }

    pub(crate) fn seal(&self) {
        match self {
            View::Clone(meta) => clone_view::seal(meta),
            View::Share(meta) => share_view::seal(meta),
        }
    }

    pub(crate) fn clone_view(&self) -> Option<Arc<CloneView>> {
        match self {
            View::Clone(meta) => Some(meta.clone()),
            _ => None,
        }
    }

    pub(crate) fn share_view(&self) -> Option<Arc<ShareView>> {
        match self {
            View::Share(meta) => Some(meta.clone()),
            _ => None,
        }
    }

    pub(crate) fn same(&self, other: &View) -> bool {
        match (self, other) {
            (View::Clone(a), View::Clone(b)) => Arc::ptr_eq(a, b),
            (View::Share(a), View::Share(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    pub(crate) fn new_pend(
        &self,
        modifiable: Modifiable,
        token: PendId,
        window: Option<(
            crate::core::memory::address::PageOffset,
            crate::core::memory::address::PageCount,
        )>,
    ) -> Result<Arc<Pend>, PendFail> {
        let authority = Pend::try_new_window(modifiable, window)?;
        if let View::Share(meta) = self {
            meta.register_pend(token, authority.clone())?;
        }
        Ok(authority)
    }

    pub(crate) fn release_pend(&self, token: PendId) {
        if let View::Share(meta) = self {
            meta.release_pend(token);
        }
    }
}
