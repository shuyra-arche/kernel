//! Allocation-free, immutable layout geometry; inclusive ends cover the top page.
use kernel_api::address::PAGE_SIZE;
pub(crate) const MAX_HART_SLOTS: usize = kernel_api::boot::platform::riscv::hart::MAX_HARTS;
pub(crate) const IMAGE_BASE: usize = 0x10000;
pub(crate) const TRAMPOLINE: usize = usize::MAX - PAGE_SIZE + 1;
pub(crate) const KERNEL_TOP: usize = TRAMPOLINE - (2 * 1024 * 1024 - PAGE_SIZE);
// Keep the page-table-friendly 2 MiB aperture while admitting only 256 frames.
pub(crate) const HART_FRAME_WINDOW_SIZE: usize = 2 * 1024 * 1024;
pub(crate) const HART_FRAME_BASE: usize = KERNEL_TOP - HART_FRAME_WINDOW_SIZE;
pub(crate) const TEAM_FRAME_WINDOW_SIZE: usize = 64 * 1024 * 1024;
pub(crate) const TEAM_FRAME_BASE: usize = HART_FRAME_BASE - TEAM_FRAME_WINDOW_SIZE;
pub(crate) const TRAP_STACK_SLOT_SIZE: usize = 64 * 1024;
pub(crate) const TRAP_STACK_BASE: usize = TEAM_FRAME_BASE - MAX_HART_SLOTS * TRAP_STACK_SLOT_SIZE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Window {
    pub(crate) base: usize,
    pub(crate) last: usize,
}
impl Window {
    fn new(base: usize, last: usize) -> Option<Self> {
        (base <= last && base.is_multiple_of(PAGE_SIZE) && last % PAGE_SIZE == PAGE_SIZE - 1)
            .then_some(Self { base, last })
    }
}

pub(crate) struct KernelLayout {
    pub(crate) normal: Window,
    pub(crate) direct: Window,
    pub(crate) resource: Window,
    pub(crate) trap: Window,
    pub(crate) team: Window,
    pub(crate) hart: Window,
    pub(crate) reserved: Window,
    pub(crate) trampoline: Window,
}
impl KernelLayout {
    pub(crate) fn new(va_bits: usize, harts: usize, ram_end: usize) -> Option<Self> {
        if !matches!(va_bits, 39 | 48 | 57) || !(1..=MAX_HART_SLOTS).contains(&harts) {
            return None;
        }
        let user_end = 1usize << (va_bits - 1);
        if ram_end == 0 || ram_end > user_end || !ram_end.is_multiple_of(PAGE_SIZE) {
            return None;
        }
        let kernel_base = usize::MAX - user_end + 1;
        let direct_end = kernel_base.checked_add(ram_end)?;
        let plan = Self {
            normal: Window::new(IMAGE_BASE, user_end - 1)?,
            direct: Window::new(kernel_base, direct_end - 1)?,
            resource: Window::new(direct_end, TRAP_STACK_BASE - 1)?,
            trap: Window::new(TRAP_STACK_BASE, TEAM_FRAME_BASE - 1)?,
            team: Window::new(TEAM_FRAME_BASE, HART_FRAME_BASE - 1)?,
            hart: Window::new(HART_FRAME_BASE, KERNEL_TOP - 1)?,
            reserved: Window::new(KERNEL_TOP, TRAMPOLINE - 1)?,
            trampoline: Window::new(TRAMPOLINE, usize::MAX)?,
        };
        let high = [
            plan.direct,
            plan.resource,
            plan.trap,
            plan.team,
            plan.hart,
            plan.reserved,
            plan.trampoline,
        ];
        if high.iter().any(|window| window.base < kernel_base)
            || high.windows(2).any(|pair| pair[0].last >= pair[1].base)
        {
            return None;
        }
        Some(plan)
    }
}
