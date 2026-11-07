#![feature(allocator_ext)]

extern crate alloc;

// Compile the production Cell itself, without RV64 instructions, fake Cell
// implementations, or changes to the production library's no_std configuration.
#[path = "../../src/core/memory/cell/mod.rs"]
mod cell;

mod ownership;
mod support;
