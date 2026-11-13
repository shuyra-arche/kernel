#![feature(allocator_ext)]

extern crate alloc;

#[allow(dead_code)]
#[path = "../../src/basis/cell/page/mod.rs"]
pub(crate) mod cell;
// This host harness imports only the portable subset of this production module.
#[allow(dead_code)]
#[path = "../support/iommu/mod.rs"]
pub(crate) mod iommu;

#[allow(dead_code, unused_imports)]
#[path = "../../src/basis/name/mod.rs"]
mod name;

#[allow(dead_code)]
#[path = "../../src/basis/nest/translation/external/state.rs"]
pub(crate) mod external;
#[allow(dead_code)]
#[path = "../../src/basis/nest/memo/publication/receipt.rs"]
pub(crate) mod publication;
#[allow(dead_code)]
#[path = "../support/iommu/target.rs"]
pub(crate) mod target;

mod basis {
    pub(crate) use crate::name;
    pub(crate) mod nest {
        pub(crate) use crate::iommu::Error as PublicationError;
        pub(crate) mod memo {
            pub(crate) use crate::publication;
        }
    }
    pub(crate) mod cell {
        pub(crate) use crate::cell as page;
    }
}

mod completion;
mod engine;
mod hardware;
#[path = "../support/lock.rs"]
mod host_lock;
mod lifecycle;
mod retirement;
#[path = "../memory/support.rs"]
mod support;
mod metal {
    pub(crate) use crate::host_lock as sync;
    pub(crate) mod memory {
        pub(crate) fn topology() -> Option<&'static crate::proof::topology::Topology> {
            None
        }
        pub(crate) fn data_visible() {}
    }
    pub(crate) mod generic {
        pub(crate) use crate::iommu::contract as iommu;
        pub(crate) use crate::proof as memory;
    }
    pub(crate) use crate::iommu;
}

#[allow(dead_code)]
#[path = "../../src/metal/generic/memory/mod.rs"]
pub mod proof;
