use crate::basis::cell::page::Owner;
use alloc::vec::Vec;
use riscv::register::satp;

use crate::basis::flat::Flat;
use crate::basis::nest::Access;
use crate::basis::nest::NestError;
use crate::basis::room::deadline;
use crate::basis::unit::team::kernel;
use crate::machine::board;
use crate::machine::hart;
use crate::machine::hart::HartId;
use crate::machine::layout::HART_FRAME_BASE;
use crate::machine::layout::TRAP_STACK_GUARD;
use crate::machine::layout::TRAP_STACK_SLOT_SHIFT;
use crate::machine::layout::TRAP_STACK_SLOT_SIZE;
use crate::machine::mmu::riscv::table::PteFlags;
use crate::machine::switcher::context::TrapContext;
use crate::machine::switcher::stack;
use crate::machine::switcher::stack::TRAP_STACK_CANARY;
use crate::machine::switcher::stack::trap_stack_base;
use crate::machine::switcher::stack::trap_stack_edge;
use crate::machine::switcher::trampoline::check_fits_page;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PageCount;
use kernel_api::address::PageIndex;

use super::attach_exact;

pub(in super::super) fn init() -> Result<(), NestError> {
    let segments = hart::hart_count();
    assert!(segments > 0, "no harts");
    assert_eq!(
        TRAP_STACK_SLOT_SIZE,
        1 << TRAP_STACK_SLOT_SHIFT,
        "trap stack segment must be 2^SHIFT"
    );
    let body_size = TRAP_STACK_SLOT_SIZE - TRAP_STACK_GUARD;
    let body_pages = body_size / PAGE_SIZE;
    let pages_per_hart = body_pages + 1;
    let need = segments
        .checked_mul(pages_per_hart)
        .and_then(|pages| pages.checked_mul(PAGE_SIZE))
        .ok_or(NestError::OutOfMemory)?;
    if board::info()
        .memory
        .free()
        .iter()
        .map(|r| r.size)
        .sum::<usize>()
        < need
    {
        return Err(NestError::OutOfMemory);
    }

    let mut boot_stacks = Vec::new();
    boot_stacks
        .try_reserve_exact(segments)
        .map_err(|_| NestError::OutOfMemory)?;
    let space = &kernel().expect("kernel team not initialized").nest;
    let flags = PteFlags::V | PteFlags::R | PteFlags::W | PteFlags::A | PteFlags::D;
    let attrs = flags & (PteFlags::U | PteFlags::G | PteFlags::A | PteFlags::D);
    for h in 0..segments {
        let boot = crate::tag!(TrapStack, crate::basis::flat::allocate_cell(Owner::Kernel)?);
        boot_stacks.push(boot);

        let mut indexed = Vec::new();
        indexed
            .try_reserve_exact(body_pages)
            .map_err(|_| NestError::OutOfMemory)?;
        for page in 0..body_pages {
            indexed.push((
                PageIndex::new(page),
                crate::tag!(TrapStack, crate::basis::flat::allocate_cell(Owner::Kernel)?),
            ));
        }
        let count = PageCount::new(indexed.len()).ok_or(NestError::NotAligned)?;
        // 主体帧先备齐，再登记占用：来源准备失败不留无人持有的地址。
        let source = Flat::from_cells(Owner::Kernel, count, indexed)?.whole();
        let body_va = trap_stack_base(HartId::new(h));
        attach_exact(&space, body_va, source, Access::from_pte(flags), attrs)?;
        unsafe {
            (body_va.as_usize() as *mut usize).write(TRAP_STACK_CANARY);
        }
    }
    stack::set_boot_stacks(boot_stacks);
    check_fits_page();

    let ksatp = satp::read();
    for h in (0..hart::hart_count()).map(HartId::new) {
        let pa = space
            .physical_at(HART_FRAME_BASE + h.get() * PAGE_SIZE)
            .expect("kernel frame not mapped");
        let frame = unsafe { &mut *(pa.as_usize() as *mut TrapContext) };
        frame.kernel_satp = ksatp;
        frame.kernel_sp = trap_stack_edge(h);
        frame.trap_handler = crate::machine::mmu::riscv::canonical(
            crate::machine::switcher::trap::trap_handler as *const () as usize,
        );
        frame.trap_stack_corrupt = TRAP_STACK_CANARY;
        frame.user_pa = pa;
        frame.user_satp = ksatp;
        frame.self_va = HART_FRAME_BASE + h.get() * PAGE_SIZE;
    }

    stack::arm_hart();
    deadline::arm(deadline::blind_deadline());
    Ok(())
}
