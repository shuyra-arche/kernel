extern crate alloc;

#[allow(dead_code)]
#[path = "../../../../src/metal/generic/mmu/internal/mod.rs"]
mod cpu;
mod metal {
    pub(crate) mod generic {
        pub(crate) mod mmu {
            pub(crate) use crate::cpu as internal;
        }
    }
}
#[path = "../../../../src/metal/riscv/mmu/internal/table/entry.rs"]
mod entry;
#[allow(dead_code)]
#[path = "../../../../src/metal/riscv/mmu/internal/table/flags.rs"]
mod flags;

mod activity;
