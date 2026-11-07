//! Initial trusted platform binding. Device translation remains Off: these engines
//! own only CQ/FQ/completion storage and do not yet implement IoSpace Hardware.
use crate::core::memory::address::PhysAddr;
use crate::core::memory::flat::{BorrowedRegion, Flat, allocator::frame};
use crate::core::memory::nest::iommu::riscv::{Engine, Phase};
use crate::core::memory::nest::{Access, Placement, RegionId};
use crate::core::work::unit::team::kernel;
use crate::machine::{board, iommu::Mmio, sync::SpinLock};
use alloc::vec::Vec;
use kernel_api::abi::call::pie::PieFail;

static ENGINES: SpinLock<Vec<Engine<Mmio>>> = SpinLock::new(Vec::new());
const STEP_BUDGET: usize = 1_000_000;

pub(super) fn init() -> Result<(), PieFail> {
    let tree = board::device_tree().ok_or(PieFail::Denied)?;
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
        return Err(PieFail::Denied);
    }
    for node in tree.all_nodes().filter(|n| {
        n.compatible()
            .is_some_and(|c| c.all().any(|s| s == "riscv,iommu"))
    }) {
        if node.reg().is_none_or(|r| r.count() != 1) {
            return Err(PieFail::Denied);
        }
    }
    let dram = board::info().dram;
    let edge = dram.base.checked_add(dram.size).ok_or(PieFail::Denied)?;
    for (index, region) in regions.iter().enumerate() {
        if region.size != 4096
            || region.base == 0
            || !region.base.is_multiple_of(4096)
            || (region.base < edge && dram.base < region.base + region.size)
            || regions[..index].iter().any(|prior| prior.overlaps(*region))
        {
            return Err(PieFail::Denied);
        }
    }
    let mut engines = ENGINES.lock();
    if !engines.is_empty() {
        return Err(PieFail::Denied);
    }
    engines
        .try_reserve_exact(regions.len())
        .map_err(|_| PieFail::OoM)?;
    let nest = &kernel().ok_or(PieFail::Denied)?.space.nest;
    for region in regions {
        // MMIO remains a borrowed physical range, not an allocator-owned Cell.
        // The mapping stays in the Kernel Nest for the lifetime of the engine.
        let borrowed = unsafe { BorrowedRegion::new(PhysAddr::from_raw(region.base), 4096) }
            .map_err(|_| PieFail::Denied)?;
        let source = Flat::from_region(borrowed)
            .map_err(|_| PieFail::OoM)?
            .whole();
        let placement =
            Placement::lowest(RegionId::Normal, 4096, 4096).map_err(|_| PieFail::Denied)?;
        let reservation = nest.reserve(placement).map_err(|_| PieFail::OoM)?;
        let access = Access::READ | Access::WRITE;
        let mapping = nest
            .attach(reservation, source, access, access)
            .map_err(|_| PieFail::Denied)?;
        let cells = [frame::cell(), frame::cell(), frame::cell()];
        let [command, fault, completion] = cells;
        let cells = [
            command.map_err(|_| PieFail::OoM)?,
            fault.map_err(|_| PieFail::OoM)?,
            completion.map_err(|_| PieFail::OoM)?,
        ];
        // SAFETY: trusted QEMU platform discovery; Kernel-only MMIO mapping and
        // all overlapping boot grants are withheld. No other MMIO owner exists.
        let registers = unsafe { Mmio::new(mapping.initial_start().as_usize()) };
        let engine = Engine::new(registers, cells).map_err(|_| PieFail::OoM)?;
        engines.push(engine); // Keep responsibility even when initialization fails.
        let engine = engines.last_mut().expect("inserted engine");
        let mut ready = false;
        for _ in 0..STEP_BUDGET {
            if engine.step().map_err(failure)? == Phase::Ready {
                ready = true;
                break;
            }
        }
        if !ready {
            return Err(PieFail::Denied);
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
            return Err(PieFail::Denied);
        }
        crate::machine::console::_write(format_args!(
            "iommu: command engine ready at {:#x}; device DMA remains Off\n",
            region.base
        ));
    }
    Ok(())
}

fn failure(error: crate::core::memory::nest::iommu::riscv::EngineError) -> PieFail {
    log::error!("iommu: command engine failure: {error:?}; hardware pages retained");
    PieFail::Denied
}

pub(super) fn shutdown() {
    for engine in ENGINES.lock().iter_mut() {
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
