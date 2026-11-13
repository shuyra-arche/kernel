use crate::address::VirtAddr;

#[kernel_api_macros::envcall(class = 2, call = CellCall)]
pub mod operations {
    #[error_domain]
    /// Memory 域（class 2）的失败词汇；码是线与内核共用的那一条轴，
    /// 新调用的失败不另起域、不换码。
    pub enum CellFail {
        /// 句柄不属于调用者、访问位非法，或来源/目标控制未获授权。
        Denied = -1,
        /// 段耗尽 / 物理帧耗尽 / 映射表备不下。
        OutOfMemory = -2,
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
    /// Allocate a lazy owning Slot collection using None(FLAT) authority.
    #[operation(slot = 0, output = crate::model::cell::SlotId, error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Create {
        pub license: crate::model::identity::PendId,
        pub allocation: crate::model::cell::Allocation,
    }
    /// Release an owning Slot handle; retained mapping obligations survive.
    #[operation(slot = 1, output = (), error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Delete {
        pub slot: crate::model::cell::SlotId,
    }
    /// Move the unique Slot holding from local to remote; no implicit association/PTE changes.
    #[operation(slot = 2, output = (), error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Push {
        pub request: super::VirtAddr,
    }
    /// Move the unique Slot holding from remote to local; no implicit association/PTE changes.
    #[operation(slot = 3, output = (), error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Pull {
        pub request: super::VirtAddr,
    }
    /// Publish the generation-qualified associated Page in CellPosition.
    #[operation(slot = 4, output = (), error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Publish {
        pub position: super::VirtAddr,
    }
    /// Close admission and retract this generation. Inspect observes completion.
    #[operation(slot = 5, output = (), error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Retract {
        pub position: super::VirtAddr,
    }
    #[operation(slot = 6, output = (), error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Protect {
        pub position: super::VirtAddr,
        pub access: u64,
    }
    /// Copy a CellInfo snapshot to out; position.generation is ignored for observation.
    #[operation(slot = 7, output = (), error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Inspect {
        pub position: super::VirtAddr,
        pub out: super::VirtAddr,
    }
    /// Split an exclusive, fully materialized Slot collection into its child level.
    #[operation(slot = 8, output = crate::model::cell::SlotId, error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Split {
        pub slot: crate::model::cell::SlotId,
    }
    /// Merge an exclusive, contiguous Slot collection into its parent level.
    #[operation(slot = 9, output = crate::model::cell::SlotId, error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Merge {
        pub slot: crate::model::cell::SlotId,
    }
    /// Associate the source Page with a mapping-scoped target using CellAssociation.
    #[operation(slot = 10, output = (), error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Insert {
        pub request: super::VirtAddr,
    }
    /// Clear an unpublished, generation-qualified mapping association.
    #[operation(slot = 11, output = (), error = CellFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Remove {
        pub position: super::VirtAddr,
    }
}
pub use operations::CellCall;
pub use operations::CellFail;
pub type CellResult<T> = Result<T, erra::Error<CellFail>>;
