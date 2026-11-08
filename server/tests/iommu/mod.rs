#![feature(allocator_ext)]

extern crate alloc;

#[allow(dead_code)]
#[path = "../../src/basis/cell/page/mod.rs"]
pub(crate) mod cell;
// This host harness imports only the portable subset of this production module.
#[allow(dead_code)]
#[path = "../support/iommu/mod.rs"]
pub(crate) mod iommu;

#[path = "../../src/basis/cell/projection/admission.rs"]
pub(crate) mod admission;

#[allow(dead_code, unused_imports)]
#[path = "../../src/basis/name/mod.rs"]
mod name;

mod basis {
    pub(crate) use crate::name;
    pub(crate) mod cell {
        pub(crate) use crate::cell as page;
        pub(crate) mod projection {
            pub(crate) use crate::admission;
        }
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
#[path = "../../src/metal/riscv/iommu/pci/endpoint.rs"]
mod pci;

#[allow(dead_code)]
#[path = "../../src/metal/board/tree/mod.rs"]
pub(crate) mod description;
mod metal {
    pub(crate) use crate::iommu;
    pub(crate) mod board {
        pub(crate) use crate::description as tree;
    }
}
