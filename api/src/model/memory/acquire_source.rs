//! `Acquire` 的取得来源：一处来源数据，不按新旧或共享方式分操作。
//!
//! 独立的保留授权**不在本枚举里**：它是同一次 `Acquire` 的另一个可选入参
//! （那一格的载荷自己说）。

use crate::model::identity::{PendId, TeamId};
use crate::model::memory::MappingId;

use super::allocation::Allocation;
use super::reference_range::ReferenceRange;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcquireSource {
    /// 新 RAM：取得完整来源持有，并建立一次性创建资格。
    Allocation(Allocation),
    /// 自己已有引用上的子范围：同来源、同授权上限、同撤销依赖。
    Reference(ReferenceRange),
    /// Share 的 Pend 与授权范围内的页窗口。
    Source {
        token: PendId,
        offset: usize,
        count: usize,
    },
    /// 目标域里已授权 Mapping 的来源范围；`team` 是定位它的域约束。
    /// MappingId 或可读 VA 本身不是凭证，还要核所属 Nest 与来源授权。
    Mapping {
        team: TeamId,
        mapping: MappingId,
        offset: usize,
        count: usize,
    },
}

impl AcquireSource {
    /// 正文里认得的标签数：越界标签即非法。
    pub(crate) const TAGS: usize = 4;

    /// 载荷宽度：最多的一支是 `Mapping`（标签 + 域 + 句柄 + 偏移 + 页数）。
    pub(crate) const WORDS: usize = 5;
}
