//! Per-Hart discovery precedes online publication. Never probe V on an
//! undeclared CPU; a missing/unsupported profile cannot receive a vector Task.
use super::{Capability, Shape};
use crate::entry::boot::facts;
use crate::metal::{
    hart::{self, HartId},
    layout::MAX_HART_SLOTS,
    sync::OnceLock,
};
static PROFILES: [OnceLock<Capability>; MAX_HART_SLOTS] =
    [const { OnceLock::new() }; MAX_HART_SLOTS];

pub(crate) fn initialize() {
    let me = hart::hart_id();
    let facts = &facts::info().hart;
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
        // Probe before online publication, with local interrupts disabled. Only
        // illegal instructions inside this probe may take its rejection path.
        "csrr t6, sstatus",
        "csrci sstatus, 2",
        "csrr t4, stvec",
        "csrr t5, sepc",
        "la a1, 4f",
        "csrw stvec, a1",
        "li t0, 0x600",
        "csrs sstatus, t0",
        "5:",
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
        "6:",
        "3:",
        "csrw stvec, t4",
        "csrw sepc, t5",
        "li t0, -1537",
        "and t6, t6, t0",
        "csrw sstatus, t6",
        "ret",
        ".balign 4",
        "4:",
        "csrr a1, scause",
        "li a2, 2",
        "bne a1, a2, 7f",
        "csrr a1, sepc",
        "la a2, 5b",
        "bltu a1, a2, 7f",
        "la a2, 6b",
        "bgeu a1, a2, 7f",
        // Reject trap-based/absent vector hardware instead of executing restore
        // with an unproved vill policy. Saved CSR routing is restored on return.
        "li a0, 0",
        "la a1, 3b",
        "csrw sepc, a1",
        "sret",
        "7:",
        "csrw stvec, t4",
        "andi a1, t4, -4",
        "jr a1",
        ".option pop",
    );
}
