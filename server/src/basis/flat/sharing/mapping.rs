use crate::basis::nest::{Nest, Span};
use alloc::sync::Arc;
use kernel_api::model::identity::PendId;

pub(super) struct HeldMap {
    pub(super) mapping: Arc<Span>,
    token: PendId,
    pub(super) placement: Arc<Nest>,
}

impl HeldMap {
    pub(super) fn holds(&self, token: PendId) -> bool {
        self.token == token
    }
}
