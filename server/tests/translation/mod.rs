extern crate alloc;

#[allow(dead_code)]
#[path = "../../src/metal/generic/mmu/mod.rs"]
mod cpu;
mod metal {
    pub(crate) mod generic {
        pub(crate) use crate::cpu as mmu;
    }
}
#[path = "../../src/metal/riscv/mmu/table/entry.rs"]
mod entry;
#[allow(dead_code)]
#[path = "../../src/metal/riscv/mmu/table/flags.rs"]
mod flags;

mod activity;
