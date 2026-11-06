//! Kernel 与用户域共享的退场原因码。
//!
//! `kernel_client::room::reap` 通过 `Reap` 操作传入原因和诊断信息。
//! `0` 表示正常结束，小整数留给用户域自己的诊断编号；ABI 保留
//! [`EXIT_PANIC`] 和 [`EXIT_FAULT`]，用于区分 panic 与内核检测到的故障。
//!
//! API 只定义共享码表。程序返回值如何转换成原因码、panic handler 和
//! 整体退出策略由平台 Runtime 决定，Client 提供终止调用机制。

/// 退场原因码——`usize`，直接就是 `Reap { reason }` 送出去的那个数。
///
/// 别名而非 newtype：它要跨 ABI（内核只当数据），而"是哪一族的编号"由**域自己的
/// 编号表**说话，不由类型说话。
pub type Reason = usize;

/// 自愿／正常结束（"没有失败要报"）。
pub const EXIT_OK: Reason = 0;

/// **域内 panic**：供平台 Runtime 的 panic handler 使用。
///
/// 取高位段、与各 bin 的启动握手编号（小整数）不重叠——读 trace 的人一眼能分出
/// "启动没走通"与"域自己炸了"。
pub const EXIT_PANIC: Reason = 0xFFFF_FF01;

/// **内核给的故障原因**：用户引起的异常、未知调用号一类，由内核的故障隔离路径落账
/// 使用的保留值；用户域不应将它用作自己的诊断编号。
pub const EXIT_FAULT: Reason = 0xFFFF_FFFF;
