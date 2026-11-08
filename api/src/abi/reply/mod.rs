//! 内核返回寄存器到操作成功载荷的转换。
//!
//! `envcall` 先按操作契约识别失败码，再将成功值交给 `FromPair` 或
//! `FromTriple`。无失败操作保留完整字的位模式，不把高位解释为错误。
//! 输入寄存器由 `Wire::unpack` 校验；这里按 Kernel 保证的返回布局还原值。

use crate::address::VirtAddr;
use crate::model::cell::MappingId;
use crate::model::cell::SlotId;
use crate::model::identity::PendId;
use crate::model::identity::TaskId;
use crate::model::identity::TeamId;

/// 从成功返回的 `(a0, a1)` 还原操作的 `Output`。
///
/// 默认双寄存器返回布局；失败码由操作的 `decode_return` 先行处理。
pub trait FromPair: Sized {
    fn from_pair(v0: usize, v1: usize) -> Self;
}

/// 从成功返回的 `(a0, a1, a2)` 还原操作的 `Output`。
///
/// 用于声明了 `reply = 3` 的操作；同一调用域可以包含两种返回宽度。
pub trait FromTriple: Sized {
    fn from_triple(v0: usize, v1: usize, v2: usize) -> Self;
}

impl FromPair for () {
    fn from_pair(_v0: usize, _v1: usize) -> Self {}
}

impl FromPair for usize {
    fn from_pair(v0: usize, _v1: usize) -> Self {
        v0
    }
}

/// `Open` 的返回：`(视图起点, 整段多大)`——一段区间的两半，故一起回。
impl FromPair for (VirtAddr, usize) {
    fn from_pair(v0: usize, v1: usize) -> Self {
        (VirtAddr(v0), v1)
    }
}

impl FromPair for SlotId {
    fn from_pair(v0: usize, _v1: usize) -> Self {
        SlotId::new(v0)
    }
}

/// `Attach` 的返回：`(关系句柄, 视图起点)`——`a0` 是身份、`a1` 是地址。
impl FromPair for (MappingId, VirtAddr) {
    fn from_pair(v0: usize, v1: usize) -> Self {
        (MappingId::new(v0), VirtAddr(v1))
    }
}

impl FromPair for u64 {
    fn from_pair(v0: usize, _v1: usize) -> Self {
        v0 as u64
    }
}

/// `Peek` 的返回：`(长度, 发送者, 队里排着几只)`。
///
/// 第三格是**队列深度**——写者据此知道"我还排着几手"（孔上可以排着至多 `QUEUE_CAP` 只手）。
/// 该操作使用三寄存器返回布局（`reply = 3`）。
impl FromTriple for (usize, TaskId, usize) {
    fn from_triple(v0: usize, v1: usize, v2: usize) -> Self {
        (v0, TaskId(v1), v2)
    }
}

impl FromPair for bool {
    fn from_pair(v0: usize, _v1: usize) -> Self {
        v0 != 0
    }
}

impl FromPair for PendId {
    fn from_pair(v0: usize, _v1: usize) -> Self {
        PendId::new(v0)
    }
}

impl FromPair for TaskId {
    fn from_pair(v0: usize, _v1: usize) -> Self {
        TaskId(v0)
    }
}

impl FromPair for TeamId {
    fn from_pair(v0: usize, _v1: usize) -> Self {
        TeamId(v0)
    }
}

mod sign;
pub use sign::SignReply;

impl FromPair for (usize, usize) {
    fn from_pair(v0: usize, v1: usize) -> Self {
        (v0, v1)
    }
}

impl FromPair for crate::model::sign::SignId {
    fn from_pair(raw: usize, _: usize) -> Self {
        Self::new(raw)
    }
}
impl FromPair for crate::model::unit::ControlId {
    fn from_pair(raw: usize, _: usize) -> Self {
        Self::new(raw)
    }
}

impl FromPair for crate::model::unit::UnitTarget {
    fn from_pair(kind: usize, id: usize) -> Self {
        assert!(
            id != 0 && id <= isize::MAX as usize,
            "invalid Unit Create identity"
        );
        match kind {
            0 => Self::Task(TaskId::new(id)),
            1 => Self::Team(TeamId::new(id)),
            _ => panic!("invalid Unit Create kind"),
        }
    }
}

mod clone_view;
