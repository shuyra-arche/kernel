extern crate alloc;

#[path = "../../src/entry/boot/devices/plan/mod.rs"]
mod plan;
// This host harness imports only the portable subset of this production module.
#[allow(dead_code)]
#[path = "../../src/machine/iommu/riscv/topology/mod.rs"]
pub(crate) mod topology;

mod authority;
mod interrupt;
// This host harness imports only the portable subset of this production module.
#[allow(dead_code)]
#[path = "../../src/machine/iommu/riscv/interrupt/plan.rs"]
mod interrupt_plan;
mod routing;
mod tree;

mod machine {
    pub(crate) mod iommu {
        pub(crate) mod riscv {
            pub(crate) use crate::topology;
        }
    }
}
