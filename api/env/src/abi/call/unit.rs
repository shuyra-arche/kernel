//! call::unit — **Unit 域（class 1：装域 / 创建 task / 血缘）**：调用表（[`UnitCall`]）与失败词汇（[`UnitFail`]）。

use crate::abi::wait::Wait;
use crate::wire::program_kind::ProgramKind;
use crate::wire::{PieToken, TaskId, TeamId, VirtAddr};

/// `UnitFail` 的结果别名。
pub type UnitResult<T> = Result<T, erra::Error<UnitFail>>;

/// A target is explicitly tagged; old TaskId-only encodings are rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitTarget {
    Task(TaskId),
    Team(TeamId),
}
impl From<TaskId> for UnitTarget {
    fn from(id: TaskId) -> Self {
        Self::Task(id)
    }
}
impl From<TeamId> for UnitTarget {
    fn from(id: TeamId) -> Self {
        Self::Team(id)
    }
}
impl crate::wire::Wire for UnitTarget {
    const WORDS: usize = 2;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        let (tag, id) = match self {
            Self::Task(id) => (0, id.get()),
            Self::Team(id) => (1, id.get()),
        };
        <usize as crate::wire::Wire>::pack(&tag, words, at);
        <usize as crate::wire::Wire>::pack(&id, words, at);
    }
    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, crate::wire::Decode> {
        let tag = <usize as crate::wire::Wire>::unpack(words, at)?;
        let id = <usize as crate::wire::Wire>::unpack(words, at)?;
        if id == 0 || id > isize::MAX as usize {
            return Err(crate::wire::Decode::Invalid);
        }
        match tag {
            0 => Ok(Self::Task(TaskId::new(id))),
            1 => Ok(Self::Team(TeamId::new(id))),
            _ => Err(crate::wire::Decode::Invalid),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum ExitCause {
    Reap = 1,
    Slay = 2,
    Cascade = 3,
    Fault = 4,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskExit {
    pub task: TaskId,
    pub cause: ExitCause,
    pub reason: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinReply {
    Pending,
    Reaped(TaskExit),
    Complete,
}
impl JoinReply {
    pub fn is_reaped(self) -> bool {
        matches!(self, Self::Reaped(_))
    }
}
impl crate::wire::FromTriple for JoinReply {
    fn from_triple(cause: usize, task: usize, reason: usize) -> Self {
        if cause == 0 {
            assert!(task == 0 && reason == 0, "invalid Pending");
            return Self::Pending;
        }
        if cause == 5 {
            assert!(task == 0 && reason == 0, "invalid Complete");
            return Self::Complete;
        }
        assert!(
            task > 0 && task <= isize::MAX as usize,
            "invalid TaskExit id"
        );
        let cause = match cause {
            1 => ExitCause::Reap,
            2 => ExitCause::Slay,
            3 => ExitCause::Cascade,
            4 => ExitCause::Fault,
            _ => panic!("invalid exit cause"),
        };
        Self::Reaped(TaskExit {
            task: TaskId::new(task),
            cause,
            reason,
        })
    }
}

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
