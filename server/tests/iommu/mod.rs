#![feature(allocator_ext)]

extern crate alloc;

#[allow(dead_code)]
#[path = "../../src/core/memory/cell/mod.rs"]
pub(crate) mod cell;
#[path = "../../src/core/memory/nest/iommu/mod.rs"]
mod iommu;

mod core {
    pub(crate) mod memory {
        pub(crate) use crate::cell;
    }
}

mod completion;
mod engine;
mod hardware;
mod lifecycle;
mod retirement;
pub(crate) use iommu::riscv::topology;
#[path = "../boot/routing/fixtures.rs"]
mod routing_fixture;
#[path = "../memory/support.rs"]
mod support;
#[path = "../boot/tree.rs"]
mod tree;
