#![allow(internal_features)]
#![feature(std_internals)]
#![feature(reentrant_lock)]
#![feature(allocator_ext)]

extern crate alloc;

// Compiles the production Flat state and Cell Page without a Nest or Room model.
#[allow(dead_code, unused_imports)]
#[path = "../../../src/basis/cell/page/mod.rs"]
pub(crate) mod cell;
use cell as page;
#[allow(dead_code, unused_imports)]
#[path = "../../../src/basis/flat/error.rs"]
mod error;
use error::FlatError;
pub(crate) mod index {
    pub(crate) use kernel_api::address::PageIndex;
}
use state::slot;

#[allow(dead_code, unused_imports)]
#[path = "../../../src/basis/name/mod.rs"]
mod name;
#[allow(dead_code, unused_imports)]
#[path = "../../support/lock.rs"]
mod host_lock;
#[allow(dead_code)]
#[path = "../../../src/metal/generic/memory/mod.rs"]
pub mod proof;
#[allow(dead_code, unused_imports)]
#[path = "../../../src/basis/cell/change.rs"]
pub(crate) mod changes;
#[allow(dead_code, unused_imports)]
#[path = "../../../src/basis/flat/storage/mod.rs"]
mod state;

#[allow(dead_code, unused_imports)]
mod basis {
    pub(crate) use crate::host_lock as sync;
    pub(crate) use crate::name;
    pub(crate) mod cell {
        pub(crate) use crate::cell as page;
        pub(crate) use crate::changes as change;
    }
    pub(crate) mod flat {
        pub(crate) use crate::error::FlatError;
    }
    pub(crate) mod pend {
        pub(crate) struct Stage;
        pub(crate) struct Provenance;
    }
}
mod metal {
    pub(crate) use crate::host_lock as sync;
    pub(crate) mod memory {
        pub(crate) fn topology() -> Option<&'static crate::proof::topology::Topology> {
            None
        }
        pub(crate) fn data_visible() {}
    }
    pub(crate) mod generic {
        pub(crate) use crate::proof as memory;
    }
}

mod admission;
#[path = "../name/allocation.rs"]
mod allocation;
#[path = "../cell/geometry.rs"]
mod geometry;
mod holding;
mod maintenance;
mod materialize;
mod residency;
mod reshape;
#[path = "../cell/support.rs"]
mod support;
