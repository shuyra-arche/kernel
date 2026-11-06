//! abi — **环境调用那一面**：调用号与载荷 codec（[`call`]）· 期限（[`wait`]）· 出口码（[`exit`]）·
//! 门闩权限（[`permission`]）。返回解码只依赖 [`crate::ecall::make_fail`]；
//! trap 与 typed effect functions 由独立 Client 承载。

pub mod call;
pub mod exit;
pub mod permission;
pub mod wait;
