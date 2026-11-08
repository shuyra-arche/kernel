use crate::basis::cell::page::Owner;
use alloc::sync::Arc;
use alloc::vec::Vec;

use erra::ResultExt;

use crate::basis::flat::BorrowedRegion;
use crate::basis::flat::Flat;
use crate::basis::flat::FlatRange;
use crate::basis::nest::Access;
use crate::basis::nest::ActiveRange;
use crate::basis::nest::Placement;
use crate::basis::nest::RegionKind;
use crate::basis::nest::{Nest, NestError};
use crate::basis::unit::team;
use crate::metal::board;
use crate::metal::hart;
use crate::metal::image::kernel_edge;
use crate::metal::layout::HART_FRAME_BASE;
use crate::metal::layout::TRAMPOLINE;
use crate::metal::layout::trampoline_pa;
use crate::metal::mmu::Attributes;
use crate::metal::riscv::mmu::mode;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PageCount;
use kernel_api::address::PageIndex;
use kernel_api::address::PhysAddr;
use kernel_api::address::VirtAddr;

mod stack;

pub(super) use stack::init as init_stacks;

use crate::basis::unit::NestResult;

unsafe extern "C" {
    static _kernel_base: u8;
    static _text_end: u8;
    static _rodata_start: u8;
}

/// 启动静态布局：低 identity 在 `Normal` 内按段挂载，高镜像、跳板、
/// hart frame 与 trap stack 按 `Exact` 登记真实占用。
pub fn init() -> NestResult<()> {
    (|| -> Result<(), NestError> {
        unsafe {
            let m = board::info();
            let dram = m.memory.envelope();

            if crate::metal::mmu::canonical(dram.range().end) > mode::upper() {
                return Err(NestError::DramOverlap);
            }

            let kernel_space = Nest::kernel()?;
            let nest = &kernel_space;

            let text_start = (&raw const _kernel_base).addr();
            let text_end = (&raw const _text_end).addr();
            let rodata_start = (&raw const _rodata_start).addr();
            let image_end = kernel_edge();
            if !(text_start < text_end && text_end <= rodata_start && rodata_start <= image_end) {
                return Err(NestError::NoRegion);
            }
            let ram_flags = Attributes::READ
                | Attributes::WRITE
                | Attributes::ACCESSED
                | Attributes::DIRTY
                | Attributes::GLOBAL;
            let ro_flags = Attributes::READ | Attributes::ACCESSED | Attributes::GLOBAL;
            let text_flags = ro_flags | Attributes::EXECUTE;
            let arch = |flags: Attributes| {
                flags
                    & (Attributes::USER
                        | Attributes::GLOBAL
                        | Attributes::ACCESSED
                        | Attributes::DIRTY)
            };

            // Each bank is split only at image permission boundaries. No
            // identity mapping is created for a hole between physical banks.
            for ram in m.memory.identity() {
                let cuts = [
                    ram.base,
                    text_start.clamp(ram.base, ram.range().end),
                    text_end.clamp(ram.base, ram.range().end),
                    rodata_start.clamp(ram.base, ram.range().end),
                    image_end.clamp(ram.base, ram.range().end),
                    ram.range().end,
                ];
                for pair in cuts.windows(2) {
                    let (start, end) = (pair[0], pair[1]);
                    if start == end {
                        continue;
                    }
                    let flags = if start >= text_start && start < text_end {
                        text_flags
                    } else if start >= rodata_start && start < image_end {
                        ro_flags
                    } else {
                        ram_flags
                    };
                    let size = end - start;
                    let access = Access::from_attributes(flags);
                    let source =
                        Flat::from_region(BorrowedRegion::new(PhysAddr::from_raw(start), size)?)?
                            .whole();
                    let grid = ActiveRange::new(
                        crate::metal::mmu::canonical(start),
                        size,
                        access,
                        access,
                    )?;
                    let lower = ActiveRange::new(
                        VirtAddr::wrap(mode::lower().as_usize() + start),
                        size,
                        access,
                        access,
                    )?;
                    let reservation =
                        nest.reserve(Placement::exact(RegionKind::Normal, grid.start, size)?)?;
                    nest.attach_shaped(reservation, source.clone(), arch(flags), &[grid])?;
                    let reservation = nest.reserve(Placement::fixed(lower.start, size)?)?;
                    nest.attach_shaped(reservation, source, arch(flags), &[lower])?;
                }
            }
            nest.advance_window(RegionKind::Normal, image_end)?;

            let tramp_flags = Attributes::READ
                | Attributes::EXECUTE
                | Attributes::ACCESSED
                | Attributes::DIRTY
                | Attributes::GLOBAL;
            let trampoline = BorrowedRegion::new(trampoline_pa(), PAGE_SIZE)?;
            let source = Flat::from_region(trampoline)?.whole();
            attach_exact(
                nest,
                TRAMPOLINE,
                source,
                Access::from_attributes(tramp_flags),
                arch(tramp_flags),
            )?;

            let n = hart::hart_count();
            for h in 0..n {
                let frame =
                    crate::tag!(HartFrame, crate::basis::flat::allocate_cell(Owner::Kernel)?);
                let mut indexed = Vec::new();
                indexed
                    .try_reserve_exact(1)
                    .map_err(|_| NestError::OutOfMemory)?;
                indexed.push((PageIndex::ZERO, frame));
                let source = Flat::from_cells(
                    Owner::Kernel,
                    PageCount::new(1).expect("one frame"),
                    indexed,
                )?
                .whole();
                let flags =
                    Attributes::READ | Attributes::WRITE | Attributes::ACCESSED | Attributes::DIRTY;
                attach_exact(
                    nest,
                    HART_FRAME_BASE + h * PAGE_SIZE,
                    source,
                    Access::from_attributes(flags),
                    arch(flags),
                )?;
            }

            team::init_kernel(kernel_space.clone())?;
            hart::mark_online();
            crate::metal::riscv::mmu::activate(kernel_space.root());
            #[cfg(debug_assertions)]
            crate::metal::layout::validate();

            Ok(())
        }
    })()
    .annotate("initializing unit (kernel space + team)")
}

/// 启动静态挂载：按 `Exact` 在 `start` 登记真实占用，再把已备好的来源挂上去。
///
/// 活动形状在登记占用之前备齐；`Static` 区只有重叠检查、没有动态窗口，
/// 所以这里不走窗口校验。
fn attach_exact(
    nest: &Arc<Nest>,
    start: VirtAddr,
    source: FlatRange,
    access: Access,
    attrs: Attributes,
) -> Result<(), NestError> {
    let size = source
        .page_count()
        .checked_bytes()
        .ok_or(NestError::NotAligned)?;
    let active = ActiveRange::new(start, size, access, access)?;
    let reservation = nest.reserve(Placement::fixed(start, size)?)?;
    nest.attach_shaped(reservation, source, attrs, &[active])?;
    Ok(())
}
