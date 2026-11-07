#![feature(allocator_ext)]

extern crate alloc;

#[allow(dead_code)]
#[path = "../../src/basis/cell/page/mod.rs"]
pub(crate) mod cell;
// This host harness imports only the portable subset of this production module.
#[allow(dead_code)]
#[path = "../support/iommu/mod.rs"]
pub(crate) mod iommu;

mod basis {
    pub(crate) mod cell {
        pub(crate) use crate::cell as page;
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

#[path = "pci/mod.rs"]
mod pci_tests;
// This host harness imports only the portable subset of this production module.
#[allow(dead_code)]
#[path = "../../src/machine/iommu/riscv/pci.rs"]
mod pci;
