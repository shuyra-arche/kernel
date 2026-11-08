//! call::pend — **Pend 域（class 7，权柄轴：许可的生死与流动）**：调用表（[`PendCall`]）与失败词汇（[`PendFail`]）。

use crate::address::VirtAddr;
use crate::model::identity::PendId;
use crate::model::identity::TaskId;
use crate::model::modifiable::Modifiable;

/// `PendFail` 的结果别名。
pub type PendResult<T> = Result<T, erra::Error<PendFail>>;

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
        /// Malformed Accord request descriptor.
        Invalid = -7,
    }
    /// Read one AccordRequest snapshot from caller memory. Slots 4 and 13 are retired.
    #[operation(slot = 14, output = super::PendId, error = PendFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Accord {
        pub request: super::VirtAddr,
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
    /// The holder retires this Pend and its descendants, returning the parent borrow
    /// only after all CPU/device access has settled. Busy retains the responsibility.
    #[operation(slot = 15, output = (), error = PendFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Return {
        pub token: super::PendId,
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
}
pub use operations::PendCall;
pub use operations::PendFail;
