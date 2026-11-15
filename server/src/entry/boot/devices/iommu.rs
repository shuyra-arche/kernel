//! Initial trusted platform binding. Device translation remains Off: these engines
//! own CQ/FQ/completion storage until a runtime device binding consumes them.
use crate::basis::cell::page::Owner;
use crate::basis::flat::BorrowedRegion;
use crate::basis::flat::Flat;
use crate::basis::nest::Access;
use crate::basis::nest::Placement;
use crate::basis::nest::RegionKind;
use crate::basis::unit::team::kernel;
use crate::metal::mmu::external::Engine;
use crate::metal::mmu::external::Phase;
use crate::metal::mmu::external::controllers::CONTROLLERS;
use crate::metal::mmu::external::controllers::Controllers;
use crate::metal::mmu::external::mmio::Mmio;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::address::PhysAddr;

const STEP_BUDGET: usize = 1_000_000;

pub(in super::super) fn init(
    description: crate::metal::platform::devices::Devices<'_>,
) -> Result<(), PendFail> {
    let regions = description.controllers;
    if let Some(route) = description.interrupt {
        let base = super::external::mapped(route.base, route.size)?;
        let controller =
            unsafe { crate::metal::interrupt::plic::Plic::new(base, route.base, route.selected) };
        let controller =
            alloc::boxed::Box::try_new(controller).map_err(|_| PendFail::OutOfMemory)?;
        unsafe { controller.configure(route.contexts, route.sources, route.queues) };
        crate::basis::room::interrupt::install(controller, route)?;
    }
    if regions.is_empty() {
        return Ok(());
    }
    {
        let mut storage = CONTROLLERS.lock();
        if storage.is_some() {
            return Err(PendFail::Denied);
        }
        *storage = Some(
            Controllers::new(regions.iter().map(|r| r.base)).map_err(|_| PendFail::OutOfMemory)?,
        );
    }
    // Boot admission runs before runtime binding. Mapping MMIO can enter Nest;
    // runtime completion takes Controllers under Nest, so preparation must not
    // retain the pool guard. Previously installed engines remain in the pool
    // on every later failure, including a failed queue start or fence.
    let nest = &kernel().ok_or(PendFail::Denied)?.nest;
    for region in regions {
        // MMIO remains a borrowed physical range, not an allocator-owned Page.
        // The mapping stays in the Kernel Nest for the lifetime of the engine.
        let borrowed = unsafe { BorrowedRegion::new(PhysAddr::from_raw(region.base), 4096) }
            .map_err(|_| PendFail::Denied)?;
        let source = Flat::from_region(borrowed)
            .map_err(|_| PendFail::OutOfMemory)?
            .whole();
        let placement =
            Placement::lowest(RegionKind::Normal, 4096, 4096).map_err(|_| PendFail::Denied)?;
        let reservation = nest.reserve(placement).map_err(|_| PendFail::OutOfMemory)?;
        let access = Access::READ | Access::WRITE;
        let mapping = nest
            .attach(reservation, source, access, access)
            .map_err(|_| PendFail::Denied)?;
        let cells = [
            crate::basis::cell::allocate(Owner::Kernel),
            crate::basis::cell::allocate(Owner::Kernel),
            crate::basis::cell::allocate(Owner::Kernel),
        ];
        let [command, fault, completion] = cells;
        let cells = [
            command.map_err(|_| PendFail::OutOfMemory)?,
            fault.map_err(|_| PendFail::OutOfMemory)?,
            completion.map_err(|_| PendFail::OutOfMemory)?,
        ];
        // SAFETY: trusted normalized platform admission; Kernel-only MMIO mapping and
        // all overlapping boot authorities are withheld. No other MMIO owner exists.
        let registers = unsafe { Mmio::new(mapping.initial_start().as_usize(), region.base) };
        let mut engine = Engine::new(registers, cells).map_err(|_| PendFail::OutOfMemory)?;
        engine.enable_interrupts().map_err(failure)?;
        let started = {
            let mut storage = CONTROLLERS.lock();
            let controllers = storage.as_mut().expect("installed controller registry");
            controllers.install(engine).map_err(|_| PendFail::Denied)?;
            let engine = controllers
                .engine(region.base)
                .map_err(|_| PendFail::Denied)?;
            start(engine)
        };
        // The regular console translates its stack buffer through Kernel Nest.
        // Neither success nor error reporting may retain Controllers.
        started.map_err(failure)?;
        crate::metal::console::_write(format_args!(
            "iommu: command engine ready at {:#x}; device DMA remains Off\n",
            region.base
        ));
    }
    Ok(())
}

fn start(engine: &mut Engine<Mmio>) -> Result<(), crate::metal::mmu::external::EngineError> {
    let mut ready = false;
    for _ in 0..STEP_BUDGET {
        if engine.step()? == Phase::Ready {
            ready = true;
            break;
        }
    }
    if !ready {
        return Err(crate::metal::mmu::external::EngineError::State);
    }
    let batch = engine.invalidate()?;
    for _ in 0..STEP_BUDGET {
        if engine.observe(batch)? {
            return Ok(());
        }
    }
    Err(crate::metal::mmu::external::EngineError::State)
}

fn failure(error: crate::metal::mmu::external::EngineError) -> PendFail {
    log::error!("iommu: command engine failure: {error:?}; hardware pages retained");
    PendFail::Denied
}

pub(in super::super) fn shutdown() {
    if let Some(route) = crate::basis::room::interrupt::external() {
        route.report();
    }
    let (closed, unsettled) = {
        let mut storage = CONTROLLERS.lock();
        let Some(controllers) = storage.as_mut() else {
            return;
        };
        let (mut closed, mut unsettled) = (0, 0);
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
            if engine.phase() == Phase::Closed {
                closed += 1;
            } else {
                unsettled += 1;
            }
        }
        (closed, unsettled)
    };
    for _ in 0..unsettled {
        log::error!("iommu: shutdown unsettled; hardware pages retained");
    }
    for _ in 0..closed {
        crate::metal::console::_write(format_args!("iommu: command engine closed\n"));
    }
}
