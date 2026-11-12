pub mod clone;
pub mod share;

use alloc::sync::Arc;
use alloc::sync::Weak;

use kernel_api::abi::call::pend::PendFail;
use kernel_api::model::identity::PendId;
use kernel_api::model::identity::TaskId;
use kernel_api::model::modifiable::Modifiable;
use kernel_api::model::pend::{PendRange, PendScope};
use kernel_api::model::view::ViewKind;

use crate::basis::pend::Pend;
use crate::basis::unit::anchor::Anchor;

use crate::basis::flat::sharing::{self, Sharing};
pub(crate) use clone::CloneView;

pub(crate) fn next_id() -> Result<usize, PendFail> {
    static NEXT: crate::basis::name::Serial =
        crate::basis::name::Serial::new(1, isize::MAX as usize);
    NEXT.allocate().ok_or(PendFail::OutOfMemory)
}

#[derive(Clone)]
pub(crate) enum View {
    Clone(Arc<CloneView>),
    Share(Arc<Sharing>),
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

    pub(crate) fn life(&self) -> Weak<Anchor> {
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
            View::Clone(meta) => clone::seal(meta),
            View::Share(meta) => sharing::seal(meta),
        }
    }

    pub(crate) fn clone_view(&self) -> Option<Arc<CloneView>> {
        match self {
            View::Clone(meta) => Some(meta.clone()),
            _ => None,
        }
    }

    pub(crate) fn share_view(&self) -> Option<Arc<Sharing>> {
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
        scope: Option<PendScope>,
    ) -> Result<Arc<Pend>, PendFail> {
        let (scope, operations) = match self {
            View::Clone(_) if scope.is_none_or(|scope| scope == PendScope::Object) => (
                PendScope::Object,
                kernel_api::model::pend::Operations::Clone(
                    kernel_api::model::pend::CloneRights::all(),
                ),
            ),
            View::Share(meta) => {
                let scope = scope.unwrap_or(PendScope::Pages(PendRange::Pages {
                    offset: 0,
                    count: meta.source().page_count().get(),
                }));
                let PendScope::Pages(PendRange::Pages { offset, count }) = scope else {
                    return Err(PendFail::Denied);
                };
                meta.source()
                    .range(
                        kernel_api::address::PageOffset::new(offset),
                        kernel_api::address::PageCount::new(count).ok_or(PendFail::Denied)?,
                    )
                    .map_err(|_| PendFail::NotAligned)?;
                (
                    scope,
                    kernel_api::model::pend::Operations::Share(
                        kernel_api::model::pend::ShareRights::all(),
                    ),
                )
            }
            _ => return Err(PendFail::Denied),
        };
        let authority = Pend::try_new_scope(
            modifiable,
            scope,
            operations,
            kernel_api::model::pend::Distributable::Yes,
        )?;
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
