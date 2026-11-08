#![feature(allocator_ext)]

extern crate alloc;

// Compile the production Page itself, without RV64 instructions, fake Page
// implementations, or changes to the production library's no_std configuration.
#[path = "../../src/basis/cell/page/mod.rs"]
pub(crate) mod cell;

#[allow(dead_code)]
#[path = "../../src/basis/flat/error.rs"]
mod error;
pub(crate) mod index {
    pub(crate) use kernel_api::address::PageIndex;
}
#[allow(dead_code)]
#[path = "../../src/basis/flat/slot.rs"]
mod slot;

#[path = "../../src/machine/mmu/riscv/synchronize/scope.rs"]
mod scope;

// Resolve production module paths using the real types, without loading the
// RV64-specific address-space machinery into the host harness.
mod basis {
    pub(crate) mod cell {
        pub(crate) use crate::cell as page;
        pub(crate) mod transfer {
            pub(crate) use crate::transfer_error::TransferError;
        }
    }
}

mod admission;
mod ownership;
mod retention;
mod support;

mod change;
#[path = "../../src/basis/nest/publication.rs"]
mod memo;

#[allow(dead_code)]
#[path = "../../src/basis/flat/state.rs"]
mod state;

#[allow(dead_code)]
#[path = "../../src/basis/flat/region.rs"]
mod region;

#[path = "../../src/basis/flat/movement.rs"]
mod movement;
mod transaction;
#[allow(dead_code)]
#[path = "../../src/basis/cell/transfer/error.rs"]
mod transfer_error;
