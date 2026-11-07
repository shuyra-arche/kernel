extern crate alloc;

#[path = "../../src/entry/boot/devices/plan.rs"]
mod plan;
#[path = "../../src/entry/boot/devices/ranges.rs"]
mod ranges;
#[path = "../../src/entry/boot/devices/region.rs"]
mod region;
#[path = "../../src/entry/boot/devices/reserved.rs"]
mod reserved;
#[path = "../../src/core/memory/nest/iommu/riscv/topology/mod.rs"]
pub(crate) mod topology;

mod core {
    pub(crate) mod memory {
        pub const PAGE_SIZE: usize = 4096;
        pub(crate) mod nest {
            pub(crate) mod iommu {
                pub(crate) mod riscv {
                    pub(crate) use crate::topology;
                }
            }
        }
    }
}

mod authority;
mod routing;
mod tree;
