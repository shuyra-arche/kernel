#![feature(allocator_ext)]

extern crate alloc;

// Compile the production Page itself, without RV64 instructions, fake Page
// implementations, or changes to the production library's no_std configuration.
#[path = "../../src/basis/cell/page/mod.rs"]
pub(crate) mod cell;
use cell as page;

#[allow(dead_code)]
#[path = "../../src/basis/flat/error.rs"]
mod error;
pub(crate) mod index {
    pub(crate) use kernel_api::address::PageIndex;
}
use error::FlatError;
use state::slot;

#[path = "../../src/metal/generic/mmu/fence.rs"]
mod scope;
mod translation;

mod evidence;
#[path = "../../src/metal/generic/memory/mod.rs"]
pub mod proof;

// Resolve production module paths using the real types, without loading the
// RV64-specific address-space machinery into the host harness.
#[allow(dead_code, unused_imports)]
#[path = "../../src/basis/name/mod.rs"]
mod name;

#[allow(dead_code)]
mod basis;

mod admission;
#[path = "../name/allocation.rs"]
mod allocation;
mod levels;
mod ownership;
mod retention;
mod support;

mod change;
#[allow(dead_code)]
#[path = "../../src/basis/cell/change.rs"]
pub(crate) mod changes;
#[allow(dead_code)]
#[path = "../../src/basis/nest/memo/publication/receipt.rs"]
mod memo;

#[allow(dead_code)]
#[path = "../../src/basis/flat/storage/mod.rs"]
mod state;

mod residency;
mod transaction;
#[allow(dead_code)]
#[path = "../../src/basis/flat/transfer/error.rs"]
mod transfer_error;

#[allow(dead_code)]
#[path = "../../src/metal/generic/iommu.rs"]
mod contract;
#[allow(dead_code)]
#[path = "../../src/basis/nest/memo/publication/error.rs"]
mod external_error;
#[allow(dead_code)]
#[path = "../../src/basis/nest/memo/publication/identity.rs"]
mod external_identity;
use external_error::Error;
#[path = "../support/lock.rs"]
mod host_lock;
mod metal {
    pub(crate) use crate::host_lock as sync;
    pub(crate) mod generic {
        pub(crate) use crate::contract as iommu;
    }
}
