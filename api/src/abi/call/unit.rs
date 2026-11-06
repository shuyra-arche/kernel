//! call::unit — **Unit 域（class 1：装域 / 创建 task / 血缘）**：调用表（[`UnitCall`]）与失败词汇（[`UnitFail`]）。

use crate::model::identity::{PieToken, TaskId, TeamId, VirtAddr};
use crate::model::unit::ProgramKind;
use crate::model::wait::Wait;

/// `UnitFail` 的结果别名。
pub type UnitResult<T> = Result<T, erra::Error<UnitFail>>;

use crate::model::unit::{JoinReply, UnitTarget};

#[kernel_api_macros::envcall(class = 1, call = UnitCall)]
pub mod operations {
    #[error_domain]
    /// Unit 域（class 1：装域 / 创建 task / 血缘）的失败词汇。
    pub enum UnitFail {
        /// 不在我 heir 里 / 名册里没有这个 id / 启动参数读不出来。
        Denied = -1,
        /// 条件未就绪（`Fall` 没等到 / `Oust` 还有没收尾的 task）。
        #[busy]
        Busy = -2,
        /// 空域、任务准备或发布所需的资源不足。
        OoM = -3,
        /// 用户态 loader 保留的镜像错误码；内核 Build 不解释镜像。
        BadImage = -4,
        /// 入口不是当前域内有效、对齐的可执行地址。
        BadEntry = -5,
    }
    #[operation(slot = 32, output = super::TeamId, error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Build {
        pub kind: super::ProgramKind,
        pub sire: super::TaskId,
    }
    #[operation(slot = 33, output = super::TaskId, error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Spawn {
        pub team: super::TeamId,
        pub entry: usize,
        pub args: super::VirtAddr,
        pub count: usize,
        pub stack: usize,
    }
    #[operation(slot = 34, output = (), error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Embark {
        pub target: super::UnitTarget,
    }
    #[operation(slot = 35, output = (), error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Debark {
        pub target: super::UnitTarget,
    }
    #[operation(slot = 36, output = (), error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Slay {
        pub target: super::UnitTarget,
    }
    #[operation(slot = 37, output = super::JoinReply, error = UnitFail, reply = 3)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Join {
        pub target: super::UnitTarget,
        pub millis: super::Wait,
        pub receive: bool,
    }
    #[operation(slot = 38, output = (), error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Oust {
        pub team: super::TeamId,
    }
    #[operation(slot = 39, output = usize, error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Scan {
        pub after: super::TeamId,
        pub buf: super::VirtAddr,
        pub capacity: usize,
    }
    #[operation(slot = 40, output = super::TaskId, infallible)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct SelfId;
    #[operation(slot = 41, output = super::TaskId, infallible)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Sire;
    #[operation(slot = 42, output = bool, error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Fall {
        pub millis: super::Wait,
    }
    #[operation(slot = 43, output = (), error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Guard {
        pub team: super::TeamId,
        pub authority: super::PieToken,
    }
    #[operation(slot = 44, output = (), error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Unguard {
        pub team: super::TeamId,
        pub authority: super::PieToken,
    }
    #[operation(slot = 45, output = super::TaskId, infallible)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Creator;
}
pub use operations::{UnitCall, UnitFail};
