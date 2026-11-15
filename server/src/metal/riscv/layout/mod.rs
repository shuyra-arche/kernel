#[cfg(debug_assertions)]
use crate::metal::mmu::internal::Address;
#[cfg(debug_assertions)]
use crate::metal::mmu::internal::mode;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PhysAddr;
use kernel_api::address::VirtAddr;

pub const IMAGE_BASE: VirtAddr = VirtAddr::wrap(plan::IMAGE_BASE);
pub(crate) const TASK_STACK_SIZE: usize = 64 * 1024;
pub(crate) const TASK_STACK_GUARD: usize = PAGE_SIZE;
pub const MAX_HART_SLOTS: usize = plan::MAX_HART_SLOTS;
pub(crate) const TRAMPOLINE: VirtAddr = VirtAddr::wrap(plan::TRAMPOLINE);
pub(crate) const KERNEL_TOP: VirtAddr = VirtAddr::wrap(plan::KERNEL_TOP);
pub(crate) const HART_FRAME_SLOTS: usize = MAX_HART_SLOTS;
pub(crate) const HART_FRAME_WINDOW_SIZE: usize = plan::HART_FRAME_WINDOW_SIZE;
pub(crate) const HART_FRAME_BASE: VirtAddr = VirtAddr::wrap(plan::HART_FRAME_BASE);
pub(crate) const TEAM_FRAME_WINDOW_SIZE: usize = plan::TEAM_FRAME_WINDOW_SIZE;
pub(crate) const TEAM_FRAME_BASE: VirtAddr = VirtAddr::wrap(plan::TEAM_FRAME_BASE);
pub(crate) const TRAP_STACK_SLOT_SIZE: usize = plan::TRAP_STACK_SLOT_SIZE;
pub(crate) const TRAP_STACK_SLOT_SHIFT: usize = 16;
pub(crate) const TRAP_STACK_GUARD: usize = PAGE_SIZE;
pub(crate) const TRAP_STACK_BASE: VirtAddr = VirtAddr::wrap(plan::TRAP_STACK_BASE);
pub(crate) fn trampoline_pa() -> PhysAddr {
    unsafe extern "C" {
        static __trampoline_start: u8;
    }
    PhysAddr::from_raw(::core::ptr::addr_of!(__trampoline_start) as usize)
}

const _: () = {
    assert!(HART_FRAME_SLOTS * PAGE_SIZE <= HART_FRAME_WINDOW_SIZE);
    assert!(TRAMPOLINE.as_usize().is_multiple_of(PAGE_SIZE));
    assert!(HART_FRAME_BASE.as_usize().is_multiple_of(PAGE_SIZE));
    assert!(KERNEL_TOP.as_usize().is_multiple_of(2 * 1024 * 1024));
    assert!(TRAMPOLINE.as_usize() - KERNEL_TOP.as_usize() == 2 * 1024 * 1024 - PAGE_SIZE);
    assert!(HART_FRAME_BASE.as_usize() + HART_FRAME_WINDOW_SIZE == KERNEL_TOP.as_usize());
    assert!(HART_FRAME_BASE.as_usize().is_multiple_of(2 * 1024 * 1024));
    assert!(TEAM_FRAME_WINDOW_SIZE.is_multiple_of(2 * 1024 * 1024));
    assert!(TEAM_FRAME_BASE.as_usize().is_multiple_of(2 * 1024 * 1024));
    assert!(TEAM_FRAME_BASE.as_usize() + TEAM_FRAME_WINDOW_SIZE == HART_FRAME_BASE.as_usize());
    assert!(TASK_STACK_SIZE.is_multiple_of(PAGE_SIZE));
    assert!(TRAP_STACK_BASE.as_usize().is_multiple_of(2 * 1024 * 1024));
    assert!(
        TRAP_STACK_BASE.as_usize() + (MAX_HART_SLOTS << TRAP_STACK_SLOT_SHIFT)
            == TEAM_FRAME_BASE.as_usize()
    );
    assert!(TRAP_STACK_SLOT_SIZE == 1usize << TRAP_STACK_SLOT_SHIFT);
    assert!(TRAP_STACK_GUARD == PAGE_SIZE);
};

#[cfg(debug_assertions)]
pub(crate) fn validate() {
    let geo = mode::geometry(mode::mode());
    let split = geo.split_bit() as usize;
    let top = 1usize << split;
    let lower = mode::lower();
    let upper = mode::upper();
    assert!(
        (3..=5).contains(&geo.levels) && geo.va_bits as usize == 12 + 9 * geo.levels as usize,
        "mode geometry incoherent: {geo:?}"
    );
    assert_eq!(
        lower.as_usize(),
        (1usize << split) | (usize::MAX << (split + 1)),
        "lower not canonical kernel base"
    );
    assert!(!lower.is_user());
    assert_eq!(upper.as_usize(), top, "upper must equal user space ceiling");
    assert!(upper.as_usize().is_multiple_of(PAGE_SIZE));
    assert!(VirtAddr::wrap(upper.as_usize() - PAGE_SIZE).is_user());
    assert!(!TRAMPOLINE.is_user());
    assert!(HART_FRAME_BASE.as_usize() < TRAMPOLINE.as_usize());
    assert!(!TEAM_FRAME_BASE.is_user());
    assert!(!TRAP_STACK_BASE.is_user());
    assert!(
        TRAP_STACK_BASE.as_usize() + (MAX_HART_SLOTS << TRAP_STACK_SLOT_SHIFT)
            == TEAM_FRAME_BASE.as_usize()
    );
}

mod plan;
pub(crate) use plan::KernelLayout;
static PLAN: crate::metal::sync::OnceLock<KernelLayout> = crate::metal::sync::OnceLock::new();

pub(crate) fn init() -> Result<(), crate::basis::nest::NestError> {
    let facts = crate::entry::boot::facts::info();
    let geo =
        crate::metal::mmu::internal::mode::geometry(crate::metal::mmu::internal::mode::mode());
    let plan = KernelLayout::new(
        geo.va_bits as usize,
        facts.hart.count(),
        facts.memory.envelope().range().end,
    )
    .ok_or(crate::basis::nest::NestError::DramOverlap)?;
    PLAN.set(plan)
        .map_err(|_| crate::basis::nest::NestError::NoRegion)
}
pub(crate) fn plan() -> &'static KernelLayout {
    PLAN.get().expect("Kernel layout not frozen")
}
