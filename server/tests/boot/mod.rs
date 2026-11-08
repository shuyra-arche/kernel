extern crate alloc;

#[path = "../../src/entry/boot/devices/plan/mod.rs"]
mod plan;
// This host harness imports only the portable subset of this production module.
#[allow(dead_code)]
#[path = "../../src/machine/iommu/riscv/topology/mod.rs"]
pub(crate) mod topology;

mod authority;
mod interrupt;
mod translation;
// This host harness imports only the portable subset of this production module.
#[allow(dead_code)]
#[path = "../../src/machine/board/harts/mod.rs"]
pub(crate) mod hart_facts;
#[allow(dead_code)]
#[path = "../../src/machine/hart/id.rs"]
mod hart_id;
mod harts;
#[allow(dead_code)]
#[path = "../../src/machine/iommu/riscv/interrupt/plan.rs"]
mod interrupt_plan;
mod memory;
#[allow(dead_code)]
#[path = "../../src/machine/board/region.rs"]
mod region;
mod routing;
mod tree;
use region::Region;
#[allow(dead_code)]
#[path = "../../src/machine/board/memory/mod.rs"]
mod memory_facts;

#[allow(dead_code)]
#[path = "../../src/machine/board/tree/mod.rs"]
pub(crate) mod description;

mod machine {
    pub(crate) mod board {
        pub(crate) use crate::description as tree;
        pub(crate) use crate::hart_facts as harts;
    }
    pub(crate) mod hart {
        pub(crate) use crate::hart_id::{FirmwareId, HartId};
    }
    pub(crate) mod iommu {
        pub(crate) mod riscv {
            pub(crate) use crate::topology;
        }
    }
}
