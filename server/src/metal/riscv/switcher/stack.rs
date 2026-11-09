use alloc::vec::Vec;
use riscv::register::sie;
use riscv::register::stvec;

use crate::basis::cell::page::Page;
use crate::metal::hart::HartId;
use crate::metal::layout::TRAP_STACK_BASE;
use crate::metal::layout::TRAP_STACK_GUARD;
use crate::metal::layout::TRAP_STACK_SLOT_SHIFT;
use crate::metal::layout::TRAP_STACK_SLOT_SIZE;
use crate::metal::switcher::trampoline::alltraps_va;
use crate::metal::sync::OnceLock;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::VirtAddr;

pub(crate) const TRAP_STACK_CANARY: usize = 0x5EED_CAFE_51A7_0000;

static BOOT_STACKS: OnceLock<Vec<Page>> = OnceLock::new();

fn trap_stack_segment(hart: usize) -> (VirtAddr, VirtAddr) {
    let base = TRAP_STACK_BASE.as_usize() + hart * TRAP_STACK_SLOT_SIZE;
    (
        crate::metal::mmu::canonical(base + TRAP_STACK_GUARD),
        crate::metal::mmu::canonical(base + TRAP_STACK_SLOT_SIZE),
    )
}

pub fn trap_stack_base(hart: HartId) -> VirtAddr {
    trap_stack_segment(hart.get()).0
}

pub fn trap_stack_edge(hart: HartId) -> VirtAddr {
    trap_stack_segment(hart.get()).1
}

#[cfg(debug_assertions)]
pub(crate) fn assert_active() {
    let sp: usize;
    unsafe { core::arch::asm!("mv {}, sp", out(reg) sp, options(nomem, nostack)) };
    let me = crate::metal::hart::hart_id();
    let base = trap_stack_base(me).get();
    let edge = trap_stack_edge(me).get();
    assert!(
        sp > base && sp <= edge,
        "idle must use the guarded hart stack"
    );
}

pub(crate) fn trap_stack_hart(sp: usize) -> Option<HartId> {
    let off = sp.checked_sub(TRAP_STACK_BASE.as_usize())?;
    let h = off >> TRAP_STACK_SLOT_SHIFT;
    if h >= crate::metal::hart::hart_count() {
        return None;
    }
    let in_seg = off & (TRAP_STACK_SLOT_SIZE - 1);
    (in_seg > TRAP_STACK_GUARD && in_seg <= TRAP_STACK_SLOT_SIZE).then_some(HartId::new(h))
}

pub(crate) fn trap_stack_guard_hart(addr: usize) -> Option<HartId> {
    let off = addr.checked_sub(TRAP_STACK_BASE.as_usize())?;
    if off & (TRAP_STACK_SLOT_SIZE - 1) < TRAP_STACK_GUARD {
        let h = off >> TRAP_STACK_SLOT_SHIFT;
        (h < crate::metal::hart::hart_count()).then_some(HartId::new(h))
    } else {
        None
    }
}

pub(crate) fn boot_stack(hart: usize) -> usize {
    BOOT_STACKS.get().expect("boot stacks not initialized")[hart].physical_address() + PAGE_SIZE
}

pub(crate) fn set_boot_stacks(boot_stacks: Vec<Page>) {
    assert!(
        BOOT_STACKS.set(boot_stacks).is_ok(),
        "boot stacks double init"
    );
}

pub fn arm_hart() {
    crate::metal::clock::initialize_hart();
    unsafe {
        stvec::write(stvec::Stvec::new(alltraps_va(), stvec::TrapMode::Direct));
        let scr = crate::metal::hart::hart_frame().as_usize();
        ::core::arch::asm!("csrw sscratch, {}", in(reg) scr);
        sie::set_stimer();
        sie::set_ssoft();
        sie::set_sext();
        crate::metal::interrupt::window::arm();
    }
}
