#![allow(internal_features)]
#![feature(std_internals)]
#![feature(reentrant_lock)]
#![feature(allocator_ext)]
extern crate alloc;

// Host exercises the production Cell without loading Flat or Nest state.
#[allow(dead_code)]
#[path = "../../../src/basis/cell/page/mod.rs"]
pub(crate) mod cell;
#[allow(dead_code, unused_imports)]
#[path = "../../../src/basis/name/mod.rs"]
mod name;
#[allow(dead_code, unused_imports)]
#[path = "../../support/lock.rs"]
mod host_lock;
#[allow(dead_code)]
#[path = "../../../src/metal/generic/memory/mod.rs"]
pub mod proof;

mod basis {
    pub(crate) use crate::metal::sync;
    pub(crate) use crate::name;
}
mod metal {
    pub(crate) use crate::host_lock as sync;
    pub(crate) mod generic {
        pub(crate) use crate::proof as memory;
    }
}

mod support;
mod ownership;
mod access;
