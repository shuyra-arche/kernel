#![allow(internal_features)]
#![feature(std_internals)]
#![feature(reentrant_lock)]
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

#[path = "../../src/metal/generic/mmu/completion.rs"]
mod completion;
mod rendezvous;
#[path = "../../src/metal/generic/mmu/fence.rs"]
mod scope;
mod translation;

mod evidence;
mod fabric;
#[path = "../../src/metal/generic/memory/mod.rs"]
pub mod proof;

// Resolve production module paths using the real types, without loading the
// RV64-specific address-space machinery into the host harness.
#[allow(dead_code, unused_imports)]
#[path = "../../src/basis/name/mod.rs"]
mod name;

#[allow(dead_code)]
#[path = "model.rs"]
mod basis;

mod admission;
#[path = "name/allocation.rs"]
mod allocation;
mod levels;
mod maintenance;
mod materialize;
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

mod reshape;
mod residency;

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
#[allow(dead_code, unused_imports)]
#[path = "../support/lock.rs"]
mod host_lock;
mod metal {
    pub(crate) use crate::host_lock as sync;
    pub(crate) mod mmu {
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
    pub(crate) mod memory {
        pub(crate) fn topology() -> Option<&'static crate::proof::topology::Topology> {
            None
        }
        pub(crate) fn data_visible() {}
    }
    pub(crate) mod generic {
        pub(crate) use crate::contract as iommu;
        pub(crate) use crate::proof as memory;
        pub(crate) mod mmu {
            pub(crate) use crate::scope as fence;
        }
    }
}

fn reshape(
    pages: &mut Vec<std::sync::Arc<cell::Page>>,
    level: kernel_api::model::cell::CellLevel,
) -> Result<Vec<std::sync::Arc<cell::Page>>, cell::PageError> {
    cell::Page::reshape_with(pages, level, |pages| {
        if pages.iter().any(|page| {
            std::sync::Arc::strong_count(page) != 1
                || std::sync::Arc::weak_count(page) != 0
                || !page.allows_return()
        }) {
            return Err(cell::PageError::Busy);
        }
        Ok(())
    })
}
