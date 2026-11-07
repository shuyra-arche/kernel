#![feature(allocator_ext)]

extern crate alloc;

// Compile the production Cell itself, without RV64 instructions, fake Cell
// implementations, or changes to the production library's no_std configuration.
#[path = "../../src/core/memory/cell/mod.rs"]
pub(crate) mod cell;

#[allow(dead_code)]
#[path = "../../src/core/memory/flat/error.rs"]
mod error;
#[allow(dead_code)]
#[path = "../../src/core/memory/address/index.rs"]
pub(crate) mod index;
#[allow(dead_code)]
#[path = "../../src/core/memory/flat/slot.rs"]
mod slot;

#[path = "../../src/core/memory/nest/synchronize/scope.rs"]
mod scope;

// Resolve production module paths using the real types, without loading the
// RV64-specific address-space machinery into the host harness.
mod core {
    pub(crate) mod memory {
        pub(crate) use crate::cell;
        pub(crate) mod address {
            pub(crate) use crate::index::PageIndex;
        }
    }
}

mod admission;
mod ownership;
mod retention;
mod support;
