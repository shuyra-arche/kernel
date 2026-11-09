//! The object a Pend authorizes. Object state lives with the object; derivation,
//! limits and revocation live only in the Pend ledger.
use super::Pend;
use crate::basis::{
    flat::sharing::Sharing,
    sign::{
        WakeKey,
        endpoint::{self, Endpoint},
    },
    unit::anchor::Anchor,
    view::{CloneView, View},
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
    None {
        owner: TaskId,
    },
    View(View),
    Flat(Arc<Sharing>),
    Sign(Arc<Endpoint>),
    Port(Arc<crate::basis::port::Port>),
    Nest {
        owner: TaskId,
        nest: Arc<crate::basis::nest::Nest>,
    },
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
            Self::Flat(meta) => Identity::Flat(meta.source().source_id()),
            Self::Sign(sign) => Identity::Sign(SignId::new(sign.id)),
            Self::Port(port) => Identity::Port(port.id()),
            Self::Nest { nest, .. } => Identity::Nest(nest.id()),
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
            Self::None { owner } | Self::Nest { owner, .. } => *owner,
            Self::Port(_) => TaskId::new(0),
            Self::Flat(meta) => meta.owner(),
            Self::View(view) => view.owner(),
            Self::Sign(sign) => sign.owner(),
        }
    }
    pub(crate) fn alive(&self) -> bool {
        match self {
            Self::None { .. } | Self::Port(_) => true,
            Self::Nest { nest, .. } => !nest.is_closed(),
            Self::Flat(meta) => meta.alive(),
            Self::View(view) => view.alive(),
            Self::Sign(sign) => sign.alive(),
        }
    }
    pub(crate) fn seal(&self) {
        match self {
            Self::None { .. } | Self::Port(_) => {}
            Self::Flat(meta) => crate::basis::flat::sharing::seal(meta),
            Self::View(view) => view.seal(),
            Self::Sign(sign) => endpoint::seal(sign),
            Self::Nest { nest, .. } => {
                if nest.is_external() {
                    let _ = nest.close_external();
                } else {
                    let _ = nest.retire();
                }
            }
        }
    }
    pub(crate) fn wake(&self) -> Option<(WakeKey, Weak<Anchor>)> {
        Some(match self {
            Self::None { .. } | Self::Port(_) => return None,
            Self::Nest { nest, .. } => (WakeKey::ExternalNest { id: nest.id() }, nest.life()),
            Self::Flat(meta) => (WakeKey::seal(ViewKind::Share, meta.id), meta.life()),
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
    pub(crate) fn share_view(&self) -> Option<Arc<Sharing>> {
        match self {
            Self::View(view) => view.share_view(),
            _ => None,
        }
    }
    /// One borrowing owner coordinates Flat management and Share access.
    pub(crate) fn memory(&self) -> Option<Arc<Sharing>> {
        match self {
            Self::Flat(meta) => Some(meta.clone()),
            _ => self.share_view(),
        }
    }
    pub(crate) fn sign(&self) -> Option<Arc<Endpoint>> {
        match self {
            Self::Sign(sign) => Some(sign.clone()),
            _ => None,
        }
    }
    pub(crate) fn port(&self) -> Option<Arc<crate::basis::port::Port>> {
        match self {
            Self::Port(port) => Some(port.clone()),
            _ => None,
        }
    }
    pub(crate) fn nest(&self) -> Option<Arc<crate::basis::nest::Nest>> {
        match self {
            Self::Nest { nest, .. } => Some(nest.clone()),
            _ => None,
        }
    }
    pub(crate) fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Flat(a), Self::Flat(b)) => Arc::ptr_eq(a, b),
            (Self::View(a), Self::View(b)) => a.same(b),
            (Self::Sign(a), Self::Sign(b)) => Arc::ptr_eq(a, b),
            (Self::Port(a), Self::Port(b)) => Arc::ptr_eq(a, b),
            (Self::Nest { nest: a, .. }, Self::Nest { nest: b, .. }) => Arc::ptr_eq(a, b),
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
            Self::Flat(meta) => {
                let authority = Pend::try_new_window(
                    modifiable,
                    window,
                    Operations::Flat(kernel_api::model::pend::FlatRights::all()),
                    Distributable::Yes,
                )?;
                meta.register_pend(token, authority.clone())?;
                Ok(authority)
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
            Self::Port(_) if modifiable == Modifiable::No && window.is_none() => {
                Pend::try_new_window(
                    modifiable,
                    None,
                    Operations::Port(kernel_api::model::pend::PortRights::all()),
                    Distributable::Yes,
                )
            }
            Self::Nest { .. } if window.is_none() => Pend::try_new_window(
                modifiable,
                None,
                Operations::Nest(kernel_api::model::pend::NestRights::all()),
                Distributable::Yes,
            ),
            _ => Err(PendFail::Denied),
        }
    }
    pub(crate) fn release_pend(&self, token: PendId) {
        crate::basis::room::interrupt::withdraw(token);
        match self {
            Self::None { .. } | Self::Port(_) | Self::Nest { .. } => {}
            Self::Flat(meta) => meta.release_pend(token),
            Self::View(view) => view.release_pend(token),
            Self::Sign(sign) => endpoint::release_authority(sign, token),
        }
    }
    pub(crate) fn changed(&self, token: PendId, routing: bool) {
        crate::basis::room::interrupt::withdraw(token);
        if let Self::Sign(sign) = self {
            if !routing {
                endpoint::release_authority(sign, token);
            }
            endpoint::authority_changed(sign);
        }
    }
}
