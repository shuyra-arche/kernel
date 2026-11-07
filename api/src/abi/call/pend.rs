//! call::pend — **Pend 域（class 7，权柄轴：许可的生死与流动）**：调用表（[`PendCall`]）与失败词汇（[`PendFail`]）。

use crate::model::identity::{Mark, PendId, TaskId, VirtAddr};
use crate::model::modifiable::Modifiable;

use crate::model::pend::PendRange;

/// `PendFail` 的结果别名。
pub type PendResult<T> = Result<T, erra::Error<PendFail>>;

use crate::model::pend::ReleaseMode;

#[kernel_api_macros::envcall(class = 7, call = PendCall)]
pub mod operations {
    #[error_domain]
    /// Pend 域（class 7：权柄轴）的失败词汇。
    pub enum PendFail {
        /// 不是开辟者 / 表里没这枚 / 类型不符。
        Denied = -1,
        /// 资源已封印。
        Dead = -2,
        /// 门闩表备不下。
        OoM = -3,
        /// Share 的 `size` / 区间非法（未页对齐）。
        NotAligned = -4,
        /// 这一枚已交出去（交回即复原）。
        HandedOver = -5,
        #[busy]
        Busy = -6,
    }
    #[operation(slot = 4, output = super::PendId, error = PendFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Accord {
        pub src: super::PendId,
        pub dst: super::TaskId,
        pub subset: super::Modifiable,
        pub mark: super::Mark,
    }
    #[operation(slot = 5, output = (), error = PendFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Narrow {
        pub token: super::PendId,
        pub subset: super::Modifiable,
    }
    #[operation(slot = 6, output = (), error = PendFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Revoke {
        pub dst: super::TaskId,
        pub token: super::PendId,
    }
    /// Revoke retires this Pend and its descendants, returning the parent borrow
    /// only after all CPU/device access has settled. Busy retains the responsibility.
    #[operation(slot = 7, output = (), error = PendFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Release {
        pub token: super::PendId,
        pub mode: super::ReleaseMode,
    }
    /// Write eight words (PendInfo) to buf. Closed resources can still be inspected.
    #[operation(slot = 8, output = (), error = PendFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Inspect {
        pub token: super::PendId,
        pub buf: super::VirtAddr,
    }
    /// Write at most capacity records in increasing token order, strictly after after.
    /// Returns the number written. Each batch observes current state, not a frozen table.
    #[operation(slot = 9, output = usize, error = PendFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Collect {
        pub after: super::PendId,
        pub buf: super::VirtAddr,
        pub capacity: usize,
    }
    #[operation(slot = 10, output = bool, error = PendFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Same {
        pub a: super::PendId,
        pub b: super::PendId,
    }
    /// 本地精确派生：目标就是调用者，范围继承父窗口或按来源起点收窄。
    #[operation(slot = 13, output = super::PendId, error = PendFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Derive {
        pub src: super::PendId,
        pub subset: super::Modifiable,
        pub mark: super::Mark,
        pub range: super::PendRange,
    }
}
pub use operations::{PendCall, PendFail};
