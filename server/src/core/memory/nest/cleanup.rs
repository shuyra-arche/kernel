use alloc::boxed::Box;
use alloc::sync::Arc;

use crate::core::memory::nest::Mapping;

/// 待清理链条上的一环：一笔已经消费、但还没收尾的关系。
///
/// 节点在发布前预留下来，接管回收时不再分配；
/// 它只保存关系的强持，占用票（退休）与 Cell / 表页的账在 Nest 自己那里。
pub(super) struct CleanupNode {
    pub(super) mapping: Option<Arc<Mapping>>,
    pub(super) next: Option<Box<CleanupNode>>,
}

impl CleanupNode {
    pub(super) const fn empty() -> Self {
        Self {
            mapping: None,
            next: None,
        }
    }
}
