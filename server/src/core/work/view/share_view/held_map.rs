use alloc::sync::Arc;

use crate::core::memory::nest::{Access, Mapping};
use crate::core::work::unit::space::Space;
use kernel_api::model::identity::PendId;

use super::relation::Relation;

/// 一项活跃关系：真实 `Mapping` 加上它的放在哪、属于哪个 token。
///
/// 来源与授权记录都不在这里：`Mapping` 自己强持来源，权限位归 `Pend`。
pub(super) struct HeldMap {
    pub(super) mapping: Arc<Mapping>,
    pub(super) token: Option<PendId>,
    pub(super) placement: Arc<Space>,
    pub(super) relation: Relation,
}

impl HeldMap {
    pub(super) fn placed_in(&self, space: &Arc<Space>) -> bool {
        Arc::ptr_eq(&self.placement, space)
    }

    pub(super) fn placed_by(&self, token: PendId) -> bool {
        self.holds(token)
    }

    pub(super) fn closable(&self, token: PendId) -> bool {
        self.placed_by(token)
    }

    pub(super) fn writable(&self) -> bool {
        self.live()
            && self.mapping.active().map_or(true, |ranges| {
                ranges
                    .iter()
                    .any(|range| range.current.contains(Access::WRITE))
            })
    }

    pub(super) fn placed(
        mapping: Arc<Mapping>,
        space: &Arc<Space>,
        token: Option<PendId>,
        relation: Relation,
    ) -> Self {
        Self {
            mapping,
            token,
            placement: space.clone(),
            relation,
        }
    }

    pub(super) fn input(&self) -> bool {
        self.relation == Relation::Input
    }

    pub(super) fn opened(&self) -> bool {
        matches!(self.relation, Relation::Input | Relation::Open)
    }

    pub(super) fn live(&self) -> bool {
        self.mapping.is_live()
    }

    pub(super) fn holds(&self, token: PendId) -> bool {
        self.token == Some(token)
    }
}
