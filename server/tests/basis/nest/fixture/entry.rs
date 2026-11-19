#![allow(internal_features)]
#![feature(std_internals)]
#![feature(reentrant_lock)]
#![feature(allocator_ext)]

extern crate alloc;

// Host contracts for real Nest Memo and retirement state with explicitly inert
// Task, Pend and hardware completion fixtures. One handoff test consumes real Flat.
#[allow(dead_code, unused_imports)]
#[path = "../../../../src/basis/cell/page/mod.rs"]
pub(crate) mod cell;
use cell as page;

#[allow(dead_code)]
#[path = "../../../../src/basis/flat/error.rs"]
mod error;
use error::FlatError;
pub(crate) mod index {
    pub(crate) use kernel_api::address::PageIndex;
}
#[path = "../../flat/holding.rs"]
mod holding;

#[path = "../../../../src/metal/generic/mmu/internal/fence.rs"]
mod scope;

#[allow(dead_code)]
#[path = "../../../../src/metal/generic/memory/mod.rs"]
pub mod proof;

// Resolve production module paths using the real types, without loading the
// RV64-specific address-space machinery into the host harness.
#[allow(dead_code, unused_imports)]
#[path = "../../../../src/basis/name/mod.rs"]
mod name;

#[allow(dead_code)]
#[path = "model.rs"]
mod basis;

#[path = "../../cell/support.rs"]
mod support;

#[path = "change.rs"]
mod change;
#[allow(dead_code)]
#[path = "../../../../src/basis/cell/change.rs"]
pub(crate) mod changes;
#[allow(dead_code)]
#[path = "../../../../src/basis/nest/memo/publication/receipt.rs"]
mod memo;

#[allow(dead_code)]
#[path = "../../../../src/basis/flat/storage/mod.rs"]
mod state;
use state::slot;

#[allow(dead_code)]
#[path = "../../../../src/metal/generic/mmu/external/mod.rs"]
mod contract;
#[allow(dead_code)]
#[path = "../../../../src/basis/nest/memo/publication/error.rs"]
mod external_error;
#[allow(dead_code)]
#[path = "../../../../src/basis/nest/memo/publication/identity.rs"]
mod external_identity;
use external_error::Error;
#[allow(dead_code, unused_imports)]
#[path = "../../../support/lock.rs"]
mod host_lock;
mod metal {
    pub(crate) use crate::host_lock as sync;
    pub(crate) mod mmu {
        pub(crate) mod internal {
            pub(crate) struct Table;
            pub(crate) struct Deferred {
                target: crate::scope::Request<usize>,
            }
            impl Deferred {
                pub(crate) fn new(target: crate::scope::Request<usize>) -> Self {
                    Self { target }
                }
                pub(crate) fn confirms(&self, target: crate::scope::Request<usize>) -> bool {
                    self.target == target
                }
            }
        }
    }
    pub(crate) mod memory {
        pub(crate) fn topology() -> Option<&'static crate::proof::topology::Topology> {
            None
        }
        pub(crate) fn data_visible() {}
    }
    pub(crate) mod generic {
        pub(crate) use crate::proof as memory;
        pub(crate) mod mmu {
            pub(crate) use crate::contract as external;
            pub(crate) mod internal {
                pub(crate) use crate::scope as fence;
            }
        }
    }
}
