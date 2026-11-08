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
use error::FlatError;
use state::slot;

#[path = "../../src/machine/mmu/riscv/synchronize/scope.rs"]
mod scope;

// Resolve production module paths using the real types, without loading the
// RV64-specific address-space machinery into the host harness.
#[allow(dead_code, unused_imports)]
#[path = "../../src/basis/name/mod.rs"]
mod name;

mod basis {
    pub(crate) use crate::name;
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
#[path = "../../src/basis/nest/mapping/publication/receipt.rs"]
mod memo;

#[allow(dead_code)]
#[path = "../../src/basis/flat/storage/mod.rs"]
mod state;

mod transaction;
#[allow(dead_code)]
#[path = "../../src/basis/cell/transfer/error.rs"]
mod transfer_error;
