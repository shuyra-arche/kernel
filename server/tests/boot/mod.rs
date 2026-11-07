extern crate alloc;

#[path = "../../src/entry/boot/devices/plan.rs"]
mod plan;
#[path = "../../src/entry/boot/devices/ranges.rs"]
mod ranges;
#[path = "../../src/entry/boot/devices/region.rs"]
mod region;
#[path = "../../src/entry/boot/devices/reserved.rs"]
mod reserved;

mod core {
    pub(crate) mod memory {
        pub const PAGE_SIZE: usize = 4096;
    }
}

mod authority;
mod tree;
