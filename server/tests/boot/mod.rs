extern crate alloc;

#[path = "../../src/entry/boot/devices/plan/mod.rs"]
mod plan;
// This host harness imports only the portable subset of this production module.
#[allow(dead_code)]
#[path = "../../src/metal/riscv/iommu/topology/mod.rs"]
pub(crate) mod topology;

mod authority;
mod interrupt;
mod translation;
// This host harness imports only the portable subset of this production module.
#[allow(dead_code)]
#[path = "../../src/metal/board/harts/mod.rs"]
pub(crate) mod hart_facts;
#[allow(dead_code)]
#[path = "../../src/metal/riscv/hart/id.rs"]
mod hart_id;
mod harts;
#[allow(dead_code)]
#[path = "../../src/metal/riscv/iommu/interrupt/plan.rs"]
mod interrupt_plan;
mod memory;
#[allow(dead_code)]
#[path = "../../src/metal/board/region.rs"]
mod region;
mod routing;
mod tree;
use region::Region;
#[allow(dead_code)]
#[path = "../../src/metal/board/memory/mod.rs"]
mod memory_facts;

#[allow(dead_code)]
#[path = "../../src/metal/board/tree/mod.rs"]
pub(crate) mod description;

mod metal {
    pub(crate) mod board {
        pub(crate) use crate::description as tree;
        pub(crate) use crate::hart_facts as harts;
    }
    pub(crate) mod hart {
        pub(crate) use crate::hart_id::{FirmwareId, HartId};
    }
    pub(crate) mod riscv {
        pub(crate) mod iommu {
            pub(crate) use crate::topology;
        }
    }
}
