//! call::pie — **Pie 域（class 7，权柄轴：许可的生死与流动）**：调用表（[`PieCall`]）与失败词汇（[`PieFail`]）。

use crate::model::identity::{Mark, PieToken, TaskId, VirtAddr};
use crate::model::permission::Permission;

use crate::model::pie::GrantRange;

/// `PieFail` 的结果别名。
pub type PieResult<T> = Result<T, erra::Error<PieFail>>;

use crate::model::pie::ReleaseMode;

#[kernel_api_macros::envcall(class = 7, call = PieCall)]
pub mod operations {
    #[error_domain]
    /// Pie 域（class 7：权柄轴）的失败词汇。
    pub enum PieFail {
        /// 不是开辟者 / 表里没这枚 / 类型不符。
        Denied = -1,
        /// 资源已封印。
        Dead = -2,
        /// 门闩表备不下。
        OoM = -3,
        /// Pole 的 `size` / 区间非法（未页对齐）。
        NotAligned = -4,
        /// 这一枚已交出去（交回即复原）。
        HandedOver = -5,
        #[busy]
        Busy = -6,
    }
    #[operation(slot = 4, output = super::PieToken, error = PieFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Accord {
        pub src: super::PieToken,
        pub dst: super::TaskId,
        pub subset: super::Permission,
        pub mark: super::Mark,
    }
    #[operation(slot = 5, output = (), error = PieFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Narrow {
        pub token: super::PieToken,
        pub subset: super::Permission,
    }
    #[operation(slot = 6, output = (), error = PieFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Revoke {
        pub dst: super::TaskId,
        pub token: super::PieToken,
    }
    /// Keep removes a borrowed, non-exclusive reference and reparents its children.
    /// Revoke removes the reference and all descendants. Both work after Seal.
    #[operation(slot = 7, output = (), error = PieFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Release {
        pub token: super::PieToken,
        pub mode: super::ReleaseMode,
    }
    /// Write seven words (PieInfo) to buf. Closed resources can still be inspected.
    #[operation(slot = 8, output = (), error = PieFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Inspect {
        pub token: super::PieToken,
        pub buf: super::VirtAddr,
    }
    /// Write at most capacity records in increasing token order, strictly after after.
    /// Returns the number written. Each batch observes current state, not a frozen table.
    #[operation(slot = 9, output = usize, error = PieFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Collect {
        pub after: super::PieToken,
        pub buf: super::VirtAddr,
        pub capacity: usize,
    }
    #[operation(slot = 10, output = bool, error = PieFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Same {
        pub a: super::PieToken,
        pub b: super::PieToken,
    }
    /// 本地精确派生：目标就是调用者，范围继承父窗口或按来源起点收窄。
    #[operation(slot = 13, output = super::PieToken, error = PieFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Derive {
        pub src: super::PieToken,
        pub subset: super::Permission,
        pub mark: super::Mark,
        pub range: super::GrantRange,
    }
}
pub use operations::{PieCall, PieFail};
