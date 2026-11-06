//! call::room — **Room 域（class 0：调度词族）**：调用表（[`RoomCall`]）与失败词汇（[`RoomFail`]）。

use crate::model::identity::{TaskId, VirtAddr};
use crate::model::wait::Wait;

/// `RoomFail` 的结果别名。
pub type RoomResult<T> = Result<T, erra::Error<RoomFail>>;

#[kernel_api_macros::envcall(class = 0, call = RoomCall)]
pub mod operations {
    #[error_domain]
    /// Room 域（class 0：调度词族）的失败词汇。
    pub enum RoomFail {
        /// 目标域 / 任务已回收（`Doom`）。
        Dead = -1,
        /// 备料失败（`Park` / `ParkUntil` / `Wait` 的等待位备不下）。
        OoM = -2,
        /// 条件未就绪（等待那一路在退化上下文里答这一枚）。
        #[busy]
        Busy = -3,
        /// 调用者没有 Doom Nole 的 FETCH 权限。
        Denied = -4,
    }
    /// 主动让出处理器（词族 starve）。
    #[operation(slot = 0, output = (), infallible)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Starve;
    /// 睡眠指定毫秒数（词族 park）——**下限**：不早于 `millis` 才可能回到本任务。
    ///
    /// 保证成对：**不早于 `millis`，且至多晚一拍**（那一拍 = 内核的失明上限
    /// `chrono::timer::BLIND_MS`；武装点已收敛为 `min(本核上限, 最近活到点)`，故只有
    /// "没有任何核能替它兑现"时才会吃满那一拍）。`millis = 0` 是"让出一拍"，
    /// **不是**探测——下限族没有三态。
    ///
    /// **域侧换算必须向上取整**：把时长换成 `millis` 时，"至少"要求 **ceil**
    /// （`500µs → 1`、`1.5ms → 2`），否则 `Park{0}` 会把一次亚毫秒睡眠静默变成"让出"。
    /// 见 `kernel_client::room::park` 。
    #[operation(slot = 1, output = (), error = RoomFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Park {
        pub millis: usize,
    }
    /// 退出当前任务；不返回，也不声明成功载荷或错误域。
    ///
    /// `reason` = **退出原因码**（数据，不是策略）：`0` = 自愿/正常结束；非 0 = 域自己的
    /// 诊断编号。内核**只记录不解释**，把它写进 trace 的 `RoomEvent::Exit`。
    ///
    /// `note` = 域自己带的一句话（`VirtAddr(0)` + `len = 0` = 无话）：**"哪里算不下去"
    /// 只有域知道**（panic 现场自带的 `file:line` 就是编译器塞进只读段的字面量，不需要
    /// 符号表），而它一旦退场，那段内存也跟着没了——故内核在入口当场 `copy_in` 一段
    /// （至多 [`NOTE_MAX`](crate::abi::call::room::NOTE_MAX) 字节，超出部分截断），**由内核自己打印**：不依赖任何服务活着
    /// （"一个算不下去的域先去求一条活路"这条是被拒绝的）。
    ///
    /// 为什么原因码与这句话长在本原语上、而不另立"panic 调用"：**"域不可续"是域的判断，
    /// 内核只需要知道"这个任务不再续跑 + 为什么"**。另立入口等于把域的策略写进 ABI，
    /// 且让"任务终止"这条不变量在 ABI 里有两个出口。
    #[operation(slot = 2u32, nonreturning)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Reap {
        pub reason: usize,
        pub note: super::VirtAddr,
        pub len: usize,
    }
    /// 事件等待（词族 wait）：key + 期限——**上限族**（三态见文件头的定式）。
    #[operation(slot = 3, output = (), error = RoomFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Wait {
        pub key: usize,
        pub millis: super::Wait,
    }
    /// 事件唤醒（词族 wake）：key；返回是否唤到人。
    #[operation(slot = 4, output = bool, infallible)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Wake {
        pub key: usize,
    }
    /// 销毁目标所属的域及其子树；`task` 是指认域的手柄。
    /// 当前任务须持有内核铸造的 Doom Nole，且带 FETCH 权限。
    /// 目标域已收尾返回 `Dead`，缺少权限返回 `Denied`。
    /// 不等待回收；等待用 [`UnitCall::Join`]。
    ///
    /// [`UnitCall::Join`]: crate::abi::call::unit::UnitCall::Join
    #[operation(slot = 5, output = (), error = RoomFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Doom {
        pub task: super::TaskId,
    }
    /// 睡到**绝对点**（词族 park，与 [`RoomCall::Park`] 成对）：`at` = 自启动基准的
    /// 纳秒标量，与 [`ChronoCall::Clock`](crate::abi::call::chrono::ChronoCall::Clock) **同基准同单位**。
    ///
    /// **下限族**（口径见文件头的定式）：**不早于 `at` 返回，且至多晚一拍**。
    /// **`at` 已过 ⇒ 当场返回、不挂起**（不是"让出一拍"——让出只留给 `Park{0}`）；
    /// 已过**不报错**：周期任务迟到是常态。
    ///
    /// 周期任务的正确写法（本原语存在的理由）：
    /// `loop { next += period; ParkUntil { at: next }; work(); }` —— 到点是绝对的，
    /// **迟到不累积**。
    #[operation(slot = 6, output = (), error = RoomFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ParkUntil {
        pub at: u64,
    }
}
pub use operations::{RoomCall, RoomFail};

/// `Reap { note }` 的上限：域带的那句话最多这么长，超出部分内核**截断**（不拒绝：
/// 一句被截断的话仍有诊断价值，而拒绝会让"域正在死"这件事多一个失败模式）。
///
/// 为什么是编译期常量：内核在 `Reap` 的入口把它拷进**栈上的定长缓冲**（退场路径不分配），
/// 上限因此必须是常量。128 字节装得下 `消息 + " at file:line:col"`（panic 现场）。
pub const NOTE_MAX: usize = 128;
