//! 一份来源授权当前归谁管：私有根，还是已经交给根 Pend。
//!
//! 两个集合分得很清楚：私有阶段已建的关系记在自己这边；交给 Share 之后
//! 关系由那个 token 下的登记管，撤销随之关闭。

use alloc::sync::{Arc, Weak};

use crate::basis::work::pend::authority::Pend;
use crate::basis::work::view::share_view::ShareView;
use kernel_api::model::identity::PendId;

#[derive(Clone)]
pub(super) enum Phase {
    /// 还没有 Share 接手。
    Private,
    /// 已由根 Pend 接手：撤销依赖那枚 Pend，后续关系交给 Share 管。
    Root {
        meta: Weak<ShareView>,
        token: PendId,
        authority: Arc<Pend>,
    },
}
