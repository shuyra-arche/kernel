//! call::memory — **Memory 域（class 2：地址空间与页）**：调用表（[`MemoryCall`]）与失败词汇（[`MemoryFail`]），
//! 以及新生命周期调用的载荷类型（一型一文件）。

mod acquire_source;
mod allocation;
mod placement;
mod placement_at;
mod placement_error;
mod reference_range;
mod view_control;
mod view_selection;

pub use acquire_source::AcquireSource;
pub use allocation::Allocation;
pub use placement::Placement;
pub use placement_at::PlacementAt;
pub use placement_error::PlacementError;
pub use reference_range::ReferenceRange;
pub use view_control::ViewControl;
pub use view_selection::ViewSelection;

use crate::wire::{Decode, FlatReference, PieToken, TeamId, ViewId, VirtAddr};

/// 环境内存调用与装载格式使用的页粒度。
pub const PAGE_SIZE: usize = 4096;

/// The RV64 ABI's byte width, independent of the build host's pointer width.
pub(super) const WORD_BYTES: usize = 8;

pub(super) fn store_words<const N: usize>(words: [usize; N], out: &mut [u8]) {
    assert_eq!(out.len(), N * WORD_BYTES);
    for (word, bytes) in words.into_iter().zip(out.chunks_exact_mut(WORD_BYTES)) {
        bytes.copy_from_slice(&(word as u64).to_le_bytes());
    }
}

pub(super) fn fetch_words<const N: usize>(bytes: &[u8]) -> Option<[usize; N]> {
    if bytes.len() != N * WORD_BYTES {
        return None;
    }
    let mut words = [0; N];
    for (word, bytes) in words.iter_mut().zip(bytes.chunks_exact(WORD_BYTES)) {
        *word = usize::try_from(u64::from_le_bytes(bytes.try_into().ok()?)).ok()?;
    }
    Some(words)
}

/// 范围坐标的唯一 checked 口径：`count` 非零，且偏移、页数与字节跨度都不溢出。
///
/// 页窗口的两处用家（[`ReferenceRange`] 与 [`ViewSelection`]）共用它，
/// 线外不接受靠溢出缩短范围的输入。
pub(super) fn pages(offset: usize, count: usize) -> Result<(), Decode> {
    if count == 0 {
        return Err(Decode::Invalid);
    }
    count.checked_mul(PAGE_SIZE).ok_or(Decode::Invalid)?;
    let end = offset.checked_add(count).ok_or(Decode::Invalid)?;
    end.checked_mul(PAGE_SIZE).ok_or(Decode::Invalid)?;
    Ok(())
}

#[kernel_api_macros::envcall(class = 2, call = MemoryCall)]
pub mod operations {
    #[error_domain]
    /// Memory 域（class 2）的失败词汇；码是线与内核共用的那一条轴，
    /// 新调用的失败不另起域、不换码。
    pub enum MemoryFail {
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
    #[operation(slot = 5, output = super::FlatReference, error = MemoryFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Acquire {
        pub source: super::AcquireSource,
        pub retention: super::PieToken,
    }
    /// 解除这一份持有；不隐式 Detach，不等待其他引用，不收回其他 View。
    #[operation(slot = 6, output = (), error = MemoryFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Release {
        pub reference: super::FlatReference,
    }
    /// 在目标地址空间里预留地址并建立翻译关系；返回 (关系句柄, 视图起点)。
    /// 内部组合 Reserve → Attach，不增加独立 Reserve 调用号。
    /// `range` 与 `placement` 分别是指向定长描述的只读用户指针，从**调用者**的
    /// 地址空间各读一次快照；`control` 是这一段映射的控制权选择。
    #[operation(slot = 7, output = (super::ViewId, super::VirtAddr), error = MemoryFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Attach {
        pub range: super::VirtAddr,
        pub team: super::TeamId,
        pub placement: super::VirtAddr,
        pub access: u64,
        pub control: super::ViewControl,
    }
    /// 按身份安全撤销选定的映射区间。
    #[operation(slot = 8, output = (), error = MemoryFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Detach {
        pub team: super::TeamId,
        pub view: super::ViewId,
        pub selection: super::ViewSelection,
    }
    /// 修改选定区间的逻辑权限。
    #[operation(slot = 9, output = (), error = MemoryFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct SetAccess {
        pub team: super::TeamId,
        pub view: super::ViewId,
        pub selection: super::ViewSelection,
        pub access: u64,
    }
    /// 遍历本 Task 的驻留引用窗口，输出每页一个小端 u64 PA。
    /// `range` 指向定长 ReferenceRange；`out` 须提供 count * 8 个可写字节。
    /// 当场验证活保留授权；Fresh 可物化，Missing 不得清零恢复。
    /// PA 只在驻留持有有效期间使用，不构成设备访问授权。
    #[operation(slot = 11, output = (), error = MemoryFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Traverse {
        pub retention: super::PieToken,
        pub range: super::VirtAddr,
        pub out: super::VirtAddr,
    }
    /// 把一份来源窗口里的一页承载交进另一份窗口的空槽（四字内联，没有描述符）。
    /// `from_page` / `to_page` 各相对**自己那份引用**的窗口，内核当场解析成来源坐标。
    /// 两端都要求调用者此刻是那份来源的管理者；源页成墓碑，目标槽拿到同一个承载。
    #[operation(slot = 10, output = (), error = MemoryFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Transfer {
        pub from: super::FlatReference,
        pub from_page: usize,
        pub to: super::FlatReference,
        pub to_page: usize,
    }
}
pub use operations::{MemoryCall, MemoryFail};
/// `MemoryFail` 的结果别名。
pub type MemoryResult<T> = Result<T, erra::Error<MemoryFail>>;
