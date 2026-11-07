use alloc::sync::Arc;

use kernel_api::{model::identity::PendId, model::modifiable::Modifiable};

use crate::core::work::pend::authority::Pend;

/// 一份授权记录：token 与它当前的权限位。
///
/// 记录里不放来源，也不放实际 `Mapping`：来源由 `ShareView` 唯一持有，
/// 该 token 下的活跃关系另存，撤销边由关系自己的 token 承担。
pub(super) struct Authority {
    token: PendId,
    authority: Arc<Pend>,
}

impl Authority {
    pub(super) fn new(token: PendId, authority: Arc<Pend>) -> Self {
        Self { token, authority }
    }

    pub(super) fn authorizes(&self, token: PendId) -> bool {
        self.token == token
    }

    /// 授权是否仍然有效（失效后 modifiable 为空）。
    pub(super) fn permitted(&self) -> bool {
        self.authority.modifiable().is_some()
    }

    pub(super) fn stores(&self) -> bool {
        self.authority
            .modifiable()
            .is_some_and(|modifiable| modifiable == Modifiable::Yes)
    }

    pub(super) fn authority(&self) -> Arc<Pend> {
        self.authority.clone()
    }
}
