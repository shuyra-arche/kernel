use alloc::sync::Arc;

use crate::basis::flat::BorrowedRegion;
use crate::basis::flat::Flat;
use crate::basis::nest::Access;
use crate::basis::nest::ActiveRange;
use crate::basis::nest::Nest;
use crate::basis::nest::NestError;
use crate::basis::nest::Placement;
use crate::basis::nest::RegionId;
use crate::basis::nest::SpaceKind;
use crate::basis::unit::life::Life;
use crate::basis::unit::team;
use crate::machine::mmu::riscv::table::PteFlags;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::VirtAddr;

use super::Space;
use super::mapping::MappingRegistry;

/// 建一个空间。
pub struct SpaceBuilder {
    kind: SpaceKind,
    kernel: bool,
}

impl SpaceBuilder {
    pub fn kernel() -> Self {
        Self {
            kind: SpaceKind::Supervisor,
            kernel: true,
        }
    }

    pub fn supervisor() -> Self {
        Self {
            kind: SpaceKind::Supervisor,
            kernel: false,
        }
    }

    pub fn user() -> Self {
        Self {
            kind: SpaceKind::User,
            kernel: false,
        }
    }

    pub fn build(self) -> Result<Space, NestError> {
        let nest = if self.kernel {
            Nest::kernel(self.kind)?
        } else {
            Nest::new(self.kind)?
        };
        let space = Space {
            nest,
            life: Life::try_new()?,
            mappings: MappingRegistry::new(),
            dma: crate::machine::iommu::retirement::Owner::new()
                .map_err(|_| NestError::OutOfMemory)?,
        };
        if !self.kernel {
            let source = team::kernel().expect("kernel team not initialized");
            seed_trampoline(&space.nest, &source.space.nest)?;
        }
        Ok(space)
    }
}

/// 把内核空间的跳板映射复制进新空间：来源是内核那一段借用的物理区间。
///
/// 来源先备齐再登记占用；挂载失败的责任按 `Nest` 的划分走。
fn seed_trampoline(target: &Arc<Nest>, source: &Nest) -> Result<(), NestError> {
    const TRAMPOLINE: VirtAddr = crate::machine::layout::TRAMPOLINE;
    let (physical, flags) = source.translate(TRAMPOLINE).ok_or(NestError::NoRegion)?;
    let region = unsafe { BorrowedRegion::new(physical, PAGE_SIZE)? };
    let source_range = Flat::from_region(region)?.whole();
    let access = Access::from_pte(flags);
    let attrs = flags & (PteFlags::U | PteFlags::G | PteFlags::A | PteFlags::D);
    // 活动形状与来源都在登记占用之前备好：准备失败不放下一笔无人持有的占用。
    let active = ActiveRange::new(TRAMPOLINE, PAGE_SIZE, access, access)?;
    let reservation = target.reserve(Placement::exact(RegionId::Static, TRAMPOLINE, PAGE_SIZE)?)?;
    target.attach_shaped(reservation, source_range, attrs, &[active])?;
    Ok(())
}
