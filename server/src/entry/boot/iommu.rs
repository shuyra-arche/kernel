//! Initial trusted platform binding. Device translation remains Off: these engines
//! own CQ/FQ/completion storage until a runtime device binding consumes them.
use crate::basis::memory::address::PhysAddr;
use crate::basis::memory::cell::Owner;
use crate::basis::memory::flat::{BorrowedRegion, Flat, allocator::frame};
use crate::basis::memory::nest::iommu::riscv::{
    Engine, Phase, controllers::Controllers, topology::Topology,
};
use crate::basis::memory::nest::{Access, Placement, RegionId};
use crate::basis::work::unit::team::kernel;
use crate::machine::{
    board,
    iommu::{Mmio, controllers::CONTROLLERS},
};
use kernel_api::abi::call::pend::PendFail;

const STEP_BUDGET: usize = 1_000_000;

pub(super) fn init() -> Result<(), PendFail> {
    let tree = board::device_tree().ok_or(PendFail::Denied)?;
    let regions = super::devices::iommu_regions(&tree)?;
    if regions.is_empty() {
        return Ok(());
    }
    // The fence-only MMIO backend has been validated for coherent QEMU virt.
    // Other platforms need an explicit coherence/order contract before binding.
    if !tree.all_nodes().next().is_some_and(|root| {
        root.compatible().is_some_and(|c| {
            c.all()
                .any(|s| matches!(s, "riscv-virtio" | "riscv-virtio,qemu"))
        })
    }) {
        return Err(PendFail::Denied);
    }
    for node in tree.all_nodes().filter(|n| {
        n.compatible()
            .is_some_and(|c| c.all().any(|s| s == "riscv,iommu"))
    }) {
        if node.reg().is_none_or(|r| r.count() != 1) {
            return Err(PendFail::Denied);
        }
    }
    let dram = board::info().dram;
    let edge = dram.base.checked_add(dram.size).ok_or(PendFail::Denied)?;
    for (index, region) in regions.iter().enumerate() {
        if region.size != 4096
            || region.base == 0
            || !region.base.is_multiple_of(4096)
            || (region.base < edge && dram.base < region.base + region.size)
            || regions[..index].iter().any(|prior| prior.overlaps(*region))
        {
            return Err(PendFail::Denied);
        }
    }
    let topology = Topology::discover(&tree).map_err(|_| PendFail::Denied)?;
    // Never silently skip reserved but disabled/unresolved hardware: its initial
    // DMA state is unknown. Reject the whole setup before starting any engine.
    if regions
        .iter()
        .any(|region| !topology.controllers().any(|base| base == region.base))
    {
        return Err(PendFail::Denied);
    }
    super::devices::validate(&tree)?;
    if regions.len() != 1 {
        return Err(PendFail::Denied);
    }
    let route = crate::machine::iommu::interrupt::plan::Plan::discover(
        &tree,
        crate::machine::hart::hart_id().get(),
        regions[0].base,
    )
    .map_err(|_| PendFail::Denied)?;
    let plic = super::pci::mapped(route.base, route.size)?;
    unsafe { crate::machine::iommu::interrupt::install(plic, route) }
        .map_err(|_| PendFail::Denied)?;
    let mut storage = CONTROLLERS.lock();
    if storage.is_some() {
        return Err(PendFail::Denied);
    }
    *storage = Some(Controllers::new(topology).map_err(|_| PendFail::OoM)?);
    let controllers = storage.as_mut().expect("installed controller registry");
    let nest = &kernel().ok_or(PendFail::Denied)?.space.nest;
    for region in regions {
        // MMIO remains a borrowed physical range, not an allocator-owned Cell.
        // The mapping stays in the Kernel Nest for the lifetime of the engine.
        let borrowed = unsafe { BorrowedRegion::new(PhysAddr::from_raw(region.base), 4096) }
            .map_err(|_| PendFail::Denied)?;
        let source = Flat::from_region(borrowed)
            .map_err(|_| PendFail::OoM)?
            .whole();
        let placement =
            Placement::lowest(RegionId::Normal, 4096, 4096).map_err(|_| PendFail::Denied)?;
        let reservation = nest.reserve(placement).map_err(|_| PendFail::OoM)?;
        let access = Access::READ | Access::WRITE;
        let mapping = nest
            .attach(reservation, source, access, access)
            .map_err(|_| PendFail::Denied)?;
        let cells = [
            frame::cell(Owner::Kernel),
            frame::cell(Owner::Kernel),
            frame::cell(Owner::Kernel),
        ];
        let [command, fault, completion] = cells;
        let cells = [
            command.map_err(|_| PendFail::OoM)?,
            fault.map_err(|_| PendFail::OoM)?,
            completion.map_err(|_| PendFail::OoM)?,
        ];
        // SAFETY: trusted QEMU platform discovery; Kernel-only MMIO mapping and
        // all overlapping boot authorities are withheld. No other MMIO owner exists.
        let registers = unsafe { Mmio::new(mapping.initial_start().as_usize(), region.base) };
        let mut engine = Engine::new(registers, cells).map_err(|_| PendFail::OoM)?;
        engine.enable_interrupts().map_err(failure)?;
        controllers.install(engine).map_err(|_| PendFail::Denied)?;
        let engine = controllers
            .engine(region.base)
            .map_err(|_| PendFail::Denied)?;
        let mut ready = false;
        for _ in 0..STEP_BUDGET {
            if engine.step().map_err(failure)? == Phase::Ready {
                ready = true;
                break;
            }
        }
        if !ready {
            return Err(PendFail::Denied);
        }
        let batch = engine.invalidate().map_err(failure)?;
        let mut completed = false;
        for _ in 0..STEP_BUDGET {
            if engine.observe(batch).map_err(failure)? {
                completed = true;
                break;
            }
        }
        if !completed {
            return Err(PendFail::Denied);
        }
        crate::machine::console::_write(format_args!(
            "iommu: command engine ready at {:#x}; device DMA remains Off\n",
            region.base
        ));
    }
    Ok(())
}

fn failure(error: crate::basis::memory::nest::iommu::riscv::EngineError) -> PendFail {
    log::error!("iommu: command engine failure: {error:?}; hardware pages retained");
    PendFail::Denied
}

pub(super) fn shutdown() {
    crate::machine::iommu::interrupt::report();
    let mut storage = CONTROLLERS.lock();
    let Some(controllers) = storage.as_mut() else {
        return;
    };
    for engine in controllers.unassigned() {
        if engine.phase() == Phase::Closed {
            continue;
        }
        if engine.stop().is_err() {
            continue;
        }
        for _ in 0..STEP_BUDGET {
            match engine.step() {
                Ok(Phase::Closed) | Err(_) => break,
                _ => {}
            }
        }
        if engine.phase() != Phase::Closed {
            log::error!("iommu: shutdown unsettled; hardware pages retained");
        } else {
            crate::machine::console::_write(format_args!("iommu: command engine closed\n"));
        }
    }
}
