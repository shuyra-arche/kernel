//! The object a Pend authorizes. Object state lives with the object; derivation,
//! limits and revocation live only in the Pend ledger.
use super::Pend;
use crate::basis::{
    sign::{
        WakeKey,
        endpoint::{self, Endpoint},
    },
    unit::anchor::Anchor,
    view::{CloneView, ShareView, View},
};
use alloc::sync::{Arc, Weak};
use kernel_api::{
    abi::call::pend::PendFail,
    address::{PageCount, PageOffset},
    model::{
        identity::{PendId, TaskId},
        modifiable::Modifiable,
        pend::{Distributable, Operations, Target as Identity, ViewTarget},
        sign::{SignId, SignRights},
        view::{ViewId, ViewKind},
    },
};

#[derive(Clone)]
pub(crate) enum Target {
    None { owner: TaskId },
    View(View),
    Sign(Arc<Endpoint>),
}
impl From<View> for Target {
    fn from(value: View) -> Self {
        Self::View(value)
    }
}
impl Target {
    pub(crate) fn identity(&self) -> Identity {
        match self {
            Self::None { .. } => Identity::None,
            Self::View(view) => Identity::View(match view.kind() {
                ViewKind::Clone => ViewTarget::Clone(ViewId::new(view.id())),
                ViewKind::Share => ViewTarget::Share(ViewId::new(view.id())),
            }),
            Self::Sign(sign) => Identity::Sign(SignId::new(sign.id)),
        }
    }
    pub(crate) fn kind(&self) -> Option<ViewKind> {
        match self {
            Self::View(view) => Some(view.kind()),
            _ => None,
        }
    }
    pub(crate) fn owner(&self) -> TaskId {
        match self {
            Self::None { owner } => *owner,
            Self::View(view) => view.owner(),
            Self::Sign(sign) => sign.owner(),
        }
    }
    pub(crate) fn alive(&self) -> bool {
        match self {
            Self::None { .. } => true,
            Self::View(view) => view.alive(),
            Self::Sign(sign) => sign.alive(),
        }
    }
    pub(crate) fn seal(&self) {
        match self {
            Self::None { .. } => {}
            Self::View(view) => view.seal(),
            Self::Sign(sign) => endpoint::seal(sign),
        }
    }
    pub(crate) fn wake(&self) -> Option<(WakeKey, Weak<Anchor>)> {
        Some(match self {
            Self::None { .. } => return None,
            Self::View(view) => (WakeKey::seal(view.kind(), view.id()), view.life()),
            Self::Sign(sign) => (endpoint::trigger_key(sign), sign.life()),
        })
    }
    pub(crate) fn clone_view(&self) -> Option<Arc<CloneView>> {
        match self {
            Self::View(view) => view.clone_view(),
            _ => None,
        }
    }
    pub(crate) fn share_view(&self) -> Option<Arc<ShareView>> {
        match self {
            Self::View(view) => view.share_view(),
            _ => None,
        }
    }
    pub(crate) fn sign(&self) -> Option<Arc<Endpoint>> {
        match self {
            Self::Sign(sign) => Some(sign.clone()),
            _ => None,
        }
    }
    pub(crate) fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::View(a), Self::View(b)) => a.same(b),
            (Self::Sign(a), Self::Sign(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
    pub(crate) fn new_pend(
        &self,
        modifiable: Modifiable,
        token: PendId,
        window: Option<(PageOffset, PageCount)>,
    ) -> Result<Arc<Pend>, PendFail> {
        match self {
            Self::None { .. } if modifiable == Modifiable::No && window.is_none() => {
                Pend::try_new_window(
                    modifiable,
                    None,
                    Operations::None(kernel_api::model::pend::CreationRights::all()),
                    Distributable::Yes,
                )
            }
            Self::View(view) => view.new_pend(modifiable, token, window),
            Self::Sign(_) if modifiable == Modifiable::No && window.is_none() => {
                Pend::try_new_window(
                    modifiable,
                    None,
                    Operations::Sign(SignRights::all()),
                    Distributable::Yes,
                )
            }
            _ => Err(PendFail::Denied),
        }
    }
    pub(crate) fn release_pend(&self, token: PendId) {
        match self {
            Self::None { .. } => {}
            Self::View(view) => view.release_pend(token),
            Self::Sign(sign) => endpoint::release_authority(sign, token),
        }
    }
    pub(crate) fn changed(&self, token: PendId, routing: bool) {
        if let Self::Sign(sign) = self {
            if !routing {
                endpoint::release_authority(sign, token);
            }
            endpoint::authority_changed(sign);
        }
    }
}
