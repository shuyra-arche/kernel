use crate::address::VirtAddr;
use crate::model::cell::AcquireSource;
use crate::model::cell::FlatReference;
use crate::model::cell::MappingId;
use crate::model::cell::MappingInfo;
use crate::model::cell::MappingSelection;
use crate::model::cell::MappingTarget;
use crate::model::identity::TeamId;

#[kernel_api_macros::envcall(class = 2, call = CellCall)]
pub mod operations {
    #[error_domain]
    /// Memory 域（class 2）的失败词汇；码是线与内核共用的那一条轴，
    /// 新调用的失败不另起域、不换码。
    pub enum CellFail {
        /// 句柄不属于调用者、访问位非法，或来源/目标控制未获授权。
        Denied = -1,
        /// 段耗尽 / 物理帧耗尽 / 映射表备不下。
        OoM = -2,
        /// 地址、范围坐标或选址描述不满足格式与对齐约束。
        NotAligned = -3,
        /// 这一段不在任何已声明的映射里。
        NoRegion = -4,
        /// 该 VA 已被映射。
        AlreadyMapped = -5,
        /// 借入页（别人的所有权）不许加宽。
        WidenDenied = -6,
        #[busy]
        Busy = -7,
        /// 移交的源页空着：没有可交出去的承载。
        SourceEmpty = -8,
        /// 移交的目标槽已经压着一个承载。
        DestinationOccupied = -9,
        /// 这一页永久不可移交：来源不是普通来源，或它的旧映射无从核实。
        NotTransferable = -10,
    }
    /// 登记一份经过授权的来源持有；返回它在本任务资源表里的句柄。
    /// `retention=NONE` 是普通取得；否则它是一枚内核认可的独立保留授权。
    #[operation(slot = 5, output = super::FlatReference, error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Acquire {
        pub source: super::AcquireSource,
        pub retention: crate::model::unit::ControlId,
    }
    /// 解除这一份持有；不隐式 Detach，不等待其他引用，不收回其他映射。
    #[operation(slot = 6, output = (), error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Release {
        pub reference: super::FlatReference,
    }
    /// Attach an authorized range from one immutable Attachment snapshot.
    /// I/O supports one 4 KiB page, Exact IOVA, Caller control and READ/READ|WRITE.
    /// Wait for Inspect::Active before DMA. Slots 7/8/10/12 are retired.
    #[operation(slot = 19, output = (super::MappingId, super::VirtAddr), error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Attach {
        pub target: super::MappingTarget,
        pub request: super::VirtAddr,
    }
    /// Withdraw the selected mapping. I/O supports Whole; success closes
    /// admission but reclamation requires Inspect::Settled or MappingRetired.
    #[operation(slot = 13, output = (), error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Detach {
        pub target: super::MappingTarget,
        pub mapping: super::MappingId,
        pub selection: super::MappingSelection,
    }
    /// 修改选定区间的逻辑权限。
    #[operation(slot = 9, output = (), error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct SetAccess {
        pub team: super::TeamId,
        pub mapping: super::MappingId,
        pub selection: super::MappingSelection,
        pub access: u64,
    }
    /// 遍历本 Task 的驻留引用窗口，输出每页一个小端 u64 PA。
    /// `range` 指向定长 ReferenceRange；`out` 须提供 count * 8 个可写字节。
    /// 当场验证活保留授权；Fresh 可物化，Missing 不得清零恢复。
    /// PA 只在驻留持有有效期间使用，不构成设备访问授权。
    #[operation(slot = 11, output = (), error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Traverse {
        pub retention: crate::model::unit::ControlId,
        pub range: super::VirtAddr,
        pub out: super::VirtAddr,
    }
    /// Give a Page to a Slot (move) or Memo (publish). `target` points to Position.
    /// A zero change starts a short change; otherwise the caller owns that receipt.
    #[operation(slot = 15, output = (), error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Push {
        pub source: super::FlatReference,
        pub page: usize,
        pub target: super::VirtAddr,
        pub change: usize,
    }
    /// Take from a Slot into the destination Slot, or withdraw one Memo.
    /// Memo Pull requires destination NONE and page zero, and grants no ownership.
    #[operation(slot = 16, output = (), error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Pull {
        pub source: super::VirtAddr,
        pub destination: super::FlatReference,
        pub page: usize,
        pub change: usize,
    }
    /// Close Page admission across calls. This does not stop existing access.
    #[operation(slot = 17, output = usize, error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Begin {
        pub source: super::FlatReference,
        pub page: usize,
    }
    /// Release this caller's change receipt; outstanding hardware obligations remain.
    #[operation(slot = 18, output = (), error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct End {
        pub change: usize,
    }
    /// Observe retained external mapping state; the target Pend is revalidated.
    #[operation(slot = 14, output = super::MappingInfo, error = CellFail, reply = 3)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Inspect {
        pub target: super::MappingTarget,
        pub mapping: super::MappingId,
    }
}
pub use operations::CellCall;
pub use operations::CellFail;
/// `CellFail` 的结果别名。
pub type CellResult<T> = Result<T, erra::Error<CellFail>>;
