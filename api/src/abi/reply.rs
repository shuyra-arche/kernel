//! 内核返回寄存器到操作成功载荷的转换。
//!
//! `envcall` 先按操作契约识别失败码，再将成功值交给 `FromPair` 或
//! `FromTriple`。无失败操作保留完整字的位模式，不把高位解释为错误。
//! 输入寄存器由 `Wire::unpack` 校验；这里按 Kernel 保证的返回布局还原值。

pub use super::call::sign::SignReply;

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

impl FromPair for u64 {
    fn from_pair(v0: usize, _v1: usize) -> Self {
        v0 as u64
    }
}

impl FromPair for bool {
    fn from_pair(v0: usize, _v1: usize) -> Self {
        v0 != 0
    }
}

impl FromPair for (usize, usize) {
    fn from_pair(v0: usize, v1: usize) -> Self {
        (v0, v1)
    }
}
