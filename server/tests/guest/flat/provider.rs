//! Test-owned crate root. Compile the unchanged production module tree without
//! adding a public Kernel API or a feature/test module to production src/.
#![no_std]
#![feature(allocator_ext)]
extern crate alloc;
#[path = "../../../src/basis/mod.rs"]
mod basis;
#[path = "../../../src/entry/mod.rs"]
pub mod entry;
#[path = "../../../src/metal/mod.rs"]
mod metal;
