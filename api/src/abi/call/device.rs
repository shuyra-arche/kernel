use crate::model::{identity::TeamId, memory::FlatReference};

/// Kernel mediation of protected PCI endpoints. IDs are Kernel inventory keys,
/// not configuration addresses; using one never substitutes for ownership.
#[kernel_api_macros::envcall(class = 13, call = DeviceCall)]
pub mod operations {
    #[error_domain]
    pub enum DeviceFail {
        Denied = -1,
        Unsupported = -2,
        Invalid = -3,
        #[busy]
        Busy = -4,
        Hardware = -5,
        OoM = -6,
    }
    /// Requires boot Device authority. Returns (id, vendor/device), or (0, 0).
    #[operation(slot = 0, output = (usize, usize), error = DeviceFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Scan {
        pub after: usize,
    }
    /// Requires boot Device authority; team=0 means caller, otherwise a child.
    /// Assignment is exclusive and one-shot. DMA remains disabled. The lease
    /// ends when the last Team member exits; new ownership requires a new Team.
    #[operation(slot = 1, output = (), error = DeviceFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Claim {
        pub device: usize,
        pub team: super::TeamId,
    }
    /// Owner-only standard immutable header dword (0, 8, 12 or 0x2c).
    #[operation(slot = 2, output = usize, error = DeviceFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Inspect {
        pub device: usize,
        pub offset: usize,
    }
    /// Owner-only BAR access. Returns (low32, high32) to preserve all 64 bits.
    #[operation(slot = 3, output = (usize, usize), error = DeviceFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Read {
        pub device: usize,
        pub offset: usize,
        pub width: usize,
    }
    /// Width is 1, 2, 4 or 8; offset aligned; value must fit width.
    #[operation(slot = 4, output = (), error = DeviceFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Write {
        pub device: usize,
        pub offset: usize,
        pub width: usize,
        pub value: u64,
    }
    /// Exclusive private 4 KiB reference, with all CPU mappings already settled.
    /// Publication is asynchronous; Status must return Active before DMA begins.
    #[operation(slot = 5, output = (), error = DeviceFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Publish {
        pub device: usize,
        pub reference: super::FlatReference,
        pub iova: usize,
        pub modifiable: bool,
    }
    /// 0=Owned, 1=Publishing, 2=Active, 3=Retiring, 4=Closed, 5=Failed.
    #[operation(slot = 6, output = usize, error = DeviceFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Status {
        pub device: usize,
    }
    /// Close admission and begin trusted retirement; does not imply completion.
    #[operation(slot = 7, output = (), error = DeviceFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Retire {
        pub device: usize,
    }

    /// Device-manager authority only: observe retirement after the owner exits.
    #[operation(slot = 8, output = usize, error = DeviceFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Observe {
        pub device: usize,
    }
    /// Owner or device-manager: retained first fault (cause, IOVA). Busy if none.
    /// Queue corruption returns Hardware; neither result proves safe retirement.
    #[operation(slot = 9, output = (usize, usize), error = DeviceFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Fault {
        pub device: usize,
    }
}
pub use operations::{DeviceCall, DeviceFail};
