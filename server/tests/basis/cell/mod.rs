#![allow(internal_features)]
#![feature(std_internals)]
#![feature(reentrant_lock)]
#![feature(allocator_ext)]
extern crate alloc;

// Host exercises the production Cell without loading Flat or Nest state.
// The production Page exports additional public-to-Kernel members not needed by this suite.
#[allow(dead_code, unused_imports)]
#[path = "../../../src/basis/cell/page/mod.rs"]
pub(crate) mod cell;
use cell as page;
#[allow(dead_code, unused_imports)]
#[path = "../../support/lock.rs"]
mod host_lock;
#[allow(dead_code, unused_imports)]
#[path = "../../../src/basis/name/mod.rs"]
mod name;
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

mod access;
#[path = "../name/allocation.rs"]
mod allocation;
mod change;
#[allow(dead_code, unused_imports)]
#[path = "../../../src/basis/cell/change.rs"]
pub(crate) mod changes;
mod geometry;
mod levels;
mod ownership;
mod support;

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
