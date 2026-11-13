use crate::basis::cell::page::Owner;
use alloc::sync::Arc;
use alloc::vec::Vec;
use kernel_api::boot::capsule::Capsule;
use kernel_api::boot::capsule::PAGE;
use kernel_api::boot::capsule::Region;

use super::Team;
use super::TeamBuilder;
use crate::basis::flat::Allocation;
use crate::basis::flat::BorrowedRegion;
use crate::basis::flat::Flat;
use crate::basis::flat::FlatRange;
use crate::basis::nest::Access;
use crate::basis::nest::Nest;
use crate::basis::nest::NestError;
use crate::basis::nest::Placement;
use crate::basis::nest::RegionKind;
use crate::basis::nest::Reservation;
use crate::basis::nest::SpaceKind;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PageCount;
use kernel_api::address::PageIndex;
use kernel_api::address::PhysAddr;
use kernel_api::address::VirtAddr;

pub(crate) fn assemble(bytes: &'static [u8]) -> Result<Arc<Team>, NestError> {
    let capsule = Capsule::parse(bytes).ok_or(NestError::NoRegion)?;
    if !(bytes.as_ptr() as usize).is_multiple_of(PAGE) {
        return Err(NestError::NotAligned);
    }
    let mut image_end = crate::metal::layout::IMAGE_BASE.as_usize();
    for i in 0..capsule.count {
        let region = capsule.region(i).ok_or(NestError::NoRegion)?;
        let size = region.pages.checked_mul(PAGE).ok_or(NestError::NoRegion)?;
        if !Nest::user_range(VirtAddr::wrap(region.va), size) {
            return Err(NestError::NoRegion);
        }
        image_end = image_end.max(region.va.checked_add(size).ok_or(NestError::NoRegion)?);
    }
    TeamBuilder::new(SpaceKind::Supervisor).spawn(|team| {
        let nest = &team.nest;
        for i in 0..capsule.count {
            let region = capsule.region(i).ok_or(NestError::NoRegion)?;
            let size = region.pages.checked_mul(PAGE).ok_or(NestError::NoRegion)?;
            let va = VirtAddr::wrap(region.va);
            let access = Access::from_wire(region.flags).ok_or(NestError::WidenDenied)?;
            let data_size = region
                .data_pages
                .checked_mul(PAGE)
                .ok_or(NestError::NoRegion)?;
            if data_size > 0 {
                // 来源先备齐：可写段拷进新帧，只读段借用镜像自身那一段物理区间。
                let source = if access.contains(Access::WRITE) {
                    image_frames(bytes, &region, data_size, team.cells.clone())?
                } else {
                    let physical = bytes
                        .as_ptr()
                        .addr()
                        .checked_add(region.payload)
                        .ok_or(NestError::NoRegion)?;
                    let borrowed =
                        unsafe { BorrowedRegion::new(PhysAddr::from_raw(physical), data_size)? };
                    Flat::from_region(borrowed)?.whole()
                };
                let reservation = reserve_at(nest, va, data_size)?;
                nest.attach(reservation, source, access, access)?;
            }
            if size > data_size {
                let bss_start = va + data_size;
                let bss_size = size - data_size;
                let source = Flat::acquire(
                    team.cells.clone(),
                    Allocation::new(bss_size / PAGE_SIZE, PAGE_SIZE)?,
                )?
                .whole();
                let reservation = reserve_at(nest, bss_start, bss_size)?;
                nest.attach(reservation, source, access, access)?;
            }
        }
        nest.advance_window(RegionKind::Normal, image_end)?;
        crate::metal::mmu::identity::sync_instructions()?;
        team.set_default_entry(capsule.entry);
        Ok(())
    })
}

/// 把镜像里的数据页拷进新帧，返回覆盖整段数据区的来源。
///
/// 帧表与来源都在占用之前备好；失败时调用方的镜像字节不动。
fn image_frames(
    bytes: &'static [u8],
    region: &Region,
    data_size: usize,
    owner: Owner,
) -> Result<FlatRange, NestError> {
    let pages = data_size / PAGE_SIZE;
    let mut indexed = Vec::new();
    indexed
        .try_reserve_exact(pages)
        .map_err(|_| NestError::OutOfMemory)?;
    for page in 0..pages {
        let frame = crate::tag!(Image, crate::basis::flat::allocate_cell(owner.clone())?);
        let start = region.payload + page * PAGE;
        unsafe {
            core::ptr::copy_nonoverlapping(
                bytes[start..start + PAGE].as_ptr(),
                frame.as_mut_ptr(),
                PAGE,
            );
        }
        indexed.push((PageIndex::new(page), frame));
    }
    let count = PageCount::new(indexed.len()).ok_or(NestError::NotAligned)?;
    Ok(Flat::from_cells(owner, count, indexed)?.whole())
}

/// 在 `Normal` 窗口内登记一笔固定跨度；不动页表。
fn reserve_at(nest: &Arc<Nest>, start: VirtAddr, size: usize) -> Result<Reservation, NestError> {
    if size == 0 || start.offset() != 0 || !size.is_multiple_of(PAGE_SIZE) {
        return Err(NestError::NotAligned);
    }
    if !nest.window_contains(RegionKind::Normal, start, size) {
        return Err(NestError::NoRegion);
    }
    nest.reserve(Placement::exact(RegionKind::Normal, start, size)?)
}
