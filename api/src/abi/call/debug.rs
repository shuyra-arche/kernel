//! call::debug — **Debug 域（class 8：借内核那支 DBCN 出口）**：调用表（[`DebugCall`]）与失败词汇（[`DebugFail`]）。

use crate::model::identity::VirtAddr;

/// `DebugFail` 的结果别名。
pub type DebugResult<T> = Result<T, erra::Error<DebugFail>>;

#[kernel_api_macros::envcall(class = 8, call = DebugCall)]
pub mod operations {
    #[error_domain]
    /// Debug 域（class 8：调试面）的失败词汇。
    pub enum DebugFail {
        /// `buf` 非法（未映射 / 不可读）/ 长度为零。
        Denied = -1,
    }
    /// 把域里的一段字节写进调试控制台（SBI DBCN，**不经过任何服务**）。
    ///
    /// `len == 0` / 区间未映射 ⇒ 负值；返**写出去的字节数**。
    ///
    /// **`len > DBCN_MAX` 是截断，不是错误**：内核那一步是 `len.min(DBCN_MAX)`
    /// （`envcall/debug.rs::put`），只搬前 `DBCN_MAX` 字节，返回值即搬走的长度——
    /// 要"一个字都不少"就调用方自己分段。**与 [`DebugCall::Get`] 的"多出即拒"
    /// 不同形**：那一格超长即拒，这一格超长即截。
    #[operation(slot = 0, output = usize, error = DebugFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Put {
        pub buf: super::VirtAddr,
        pub len: usize,
    }
    /// 从调试控制台读一段字节写进域：内核一块栈暂存 → `Dbcn::ConsoleRead` → 写回域。
    ///
    /// **不阻塞**：没数据时**当场**返 0 字节——**不是**"等到至少读到一个字节"。
    /// 这是实测口径（OpenSBI v1.9 / QEMU virt；内核侧见
    /// `kernel/src/runtime/switcher/envcall/debug.rs::get`）。拿到 0 的调用方必须自己让一拍
    /// 或等中断，**别立刻再问**（域自己的判断，内核不替它决定）。
    ///
    /// 返**实际写进域的字节数**（`len > DBCN_MAX` ⇒ 负值，不截断）。
    #[operation(slot = 1, output = usize, error = DebugFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Get {
        pub buf: super::VirtAddr,
        pub len: usize,
    }
    /// 开关**报文对账**：此后每次一次往返都把"推出去那一帧"与"收回来那一帧"的前
    /// [`DBCN_MAX`](crate::abi::call::debug::DBCN_MAX) 字节以十六进制打进调试控制台。
    ///
    /// 为什么它是 ABI 的一格而不是一个环境变量：布局错位这类病**只有真实字节能证**，
    /// "读代码推布局"无济于事。
    #[operation(slot = 2, output = (), infallible)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct SetTrace {
        pub on: usize,
    }
}
pub use operations::{DebugCall, DebugFail};

/// [`DebugCall`] 那两格一次能搬的字节数上限。
///
/// 为什么是编译期常量：内核在入口把它拷进**栈上的定长缓冲**（诊断路径不分配，与
/// [`NOTE_MAX`](crate::abi::call::room::NOTE_MAX) 同一条理由）。256 够一句 `file:line + 消息`，也够敲一条命令。
pub const DBCN_MAX: usize = 256;
