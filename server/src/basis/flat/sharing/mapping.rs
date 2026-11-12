use crate::basis::nest::{Nest, Span};
use alloc::sync::Arc;
use kernel_api::model::identity::PendId;

pub(super) struct HeldMap {
    pub(super) mapping: Arc<Span>,
    token: PendId,
    pub(super) placement: Arc<Nest>,
}

impl HeldMap {
    pub(super) fn placed_in(&self, space: &Arc<Nest>) -> bool {
        Arc::ptr_eq(&self.placement, space)
    }

    pub(super) fn placed(mapping: Arc<Span>, space: &Arc<Nest>, token: PendId) -> Self {
        Self {
            mapping,
            token,
            placement: space.clone(),
        }
    }

    pub(super) fn live(&self) -> bool {
        self.mapping.is_live()
    }

    pub(super) fn holds(&self, token: PendId) -> bool {
        self.token == token
    }
}
