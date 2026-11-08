//! Per-Hart discovery precedes online publication. Never probe V on an
//! undeclared CPU; a missing/unsupported profile cannot receive a vector Task.
use super::{Capability, Shape};
use crate::machine::{
    board,
    hart::{self, HartId},
    layout::MAX_HART_SLOTS,
    sync::OnceLock,
};
static PROFILES: [OnceLock<Capability>; MAX_HART_SLOTS] =
    [const { OnceLock::new() }; MAX_HART_SLOTS];

pub(crate) fn initialize() {
    let me = hart::hart_id();
    let facts = &board::info().hart;
    let vector = if facts.vector(me) {
        // The firmware declaration grants permission to execute V 1.0 probes.
        // VLEN is read from hardware; vsetvl establishes the supported SEWs.
        let encoded = unsafe { probe() };
        Shape::checked(encoded >> 8, encoded as u8)
    } else {
        None
    };
    assert!(
        PROFILES[me.get()]
            .set(Capability {
                float: facts.floating(me),
                vector
            })
            .is_ok(),
        "duplicate Hart extension profile"
    );
}

pub(crate) fn capability(hart: HartId) -> Capability {
    PROFILES[hart.get()].get().copied().unwrap_or(Capability {
        float: false,
        vector: None,
    })
}

#[unsafe(naked)]
unsafe extern "C" fn probe() -> usize {
    core::arch::naked_asm!(
        ".option push",
        ".option arch, +v",
        "li t0, 0x600",
        "csrs sstatus, t0",
        "csrr a0, vlenb",
        "slli a0, a0, 8",
        "li t1, 0",
        "li t3, 1",
        "1:",
        "vsetvl t2, zero, t1",
        "csrr t2, vtype",
        "bltz t2, 2f",
        "or a0, a0, t3",
        "2:",
        "addi t1, t1, 8",
        "slli t3, t3, 1",
        "li t2, 64",
        "bne t1, t2, 1b",
        "csrw vstart, zero",
        "csrw vcsr, zero",
        "csrc sstatus, t0",
        "ret",
        ".option pop",
    );
}
