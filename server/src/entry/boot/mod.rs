pub(crate) mod devices;
pub(crate) mod error;
pub(crate) mod facts;
mod init;
pub(crate) mod resources;
mod space;

pub use crate::metal::generic::iommu::{HardwareError, Quiescence, Retirement, RetirementScope};
// Next-memory proof interface; exposing its types installs no backend.
pub use crate::metal::generic::memory;
pub use devices::{ExternalRoute, MapMmio};
pub use error::BootError;
pub use error::MapOperation;
pub use error::ResourceOperation;
pub use error::fail;
pub use init::{
    bind_external, configure, grant_interrupt, grant_mmio, init, map_mmio, qualify_memory,
};

use alloc::sync::Arc;

use crate::basis::flat::BorrowedRegion;
use crate::basis::flat::Flat;
use crate::basis::nest::Access;
use crate::basis::nest::Nest;
use crate::basis::nest::NestError;
use crate::basis::nest::Placement;
use crate::basis::nest::RegionKind;
use crate::basis::room::scheduler;
use crate::basis::unit::team::kernel;
use crate::metal::hart;
use crate::metal::hart::HartId;
use crate::metal::image::ROOT_STACK_CANARY;
use crate::metal::image::root_stack_base;
use crate::metal::mmu::identity;
use crate::metal::switcher::context::TrapContext;
use crate::metal::switcher::stack::arm_hart;
use crate::metal::switcher::stack::boot_stack;
use crate::metal::switcher::trampoline::restore;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PhysAddr;
use kernel_api::address::VirtAddr;

/// Executing image and ROOT stack reservation, independent of board layout.
pub fn image_region() -> kernel_api::boot::platform::Region {
    unsafe extern "C" {
        static _kernel_base: u8;
    }
    let base = (&raw const _kernel_base).addr();
    kernel_api::boot::platform::Region::new(base, crate::metal::image::root_stack_edge() - base)
}

pub fn start() -> Result<(), BootError> {
    if !devices::ready() {
        return Err(BootError::Description("devices not configured"));
    }
    scheduler::init().map_err(BootError::Scheduler)?;
    #[cfg(feature = "latency")]
    crate::metal::measure::init().map_err(BootError::Scheduler)?;
    register_runtime_hooks();

    if let Some(entry) = spawn_entry()? {
        crate::basis::room::shutdown::watch(&entry);
        crate::entry::observe::start()?;
    }

    crate::basis::unit::lifecycle::tally::rooted();

    boot_harts()?;

    let boot_guard = unsafe { (root_stack_base() as *const usize).read() };
    assert!(
        boot_guard == ROOT_STACK_CANARY,
        "ROOT stack overflow during boot: canary corrupted {boot_guard:#x}",
    );
    Ok(())
}

pub fn run() -> ! {
    // Secondary harts enter with a one-page physical bootstrap stack. Idle
    // scheduling can retire I/O before that hart ever runs a Task, so it must
    // already use the full guarded stack rather than waiting for its first trap.
    let top = crate::metal::switcher::stack::trap_stack_edge(hart::hart_id()).get();
    unsafe { crate::metal::riscv::boot::enter_idle(top, run_on_stack) }
}

extern "C" fn run_on_stack() -> ! {
    restore(scheduler::run())
}

pub(crate) fn register_runtime_hooks() {
    crate::metal::interrupt::window::bind(crate::entry::trap::service);
    use crate::basis::room::shutdown;

    static EXIT_HOOKS: &[fn(&alloc::sync::Arc<crate::basis::unit::task::Task>)] = &[
        crate::basis::unit::lifecycle::doom,
        crate::basis::pend::doom,
    ];
    crate::basis::unit::lifecycle::hook(EXIT_HOOKS);

    const SHUTDOWN_HOOKS: &[fn()] = &[
        devices::shutdown_iommu,
        crate::entry::report::shutdown::report,
        crate::basis::room::scheduler::rip,
        crate::metal::allocator::block::flush,
    ];
    shutdown::hook(
        SHUTDOWN_HOOKS,
        crate::entry::report::shutdown::waiting,
        crate::entry::report::shutdown::stalled,
    );
}

fn spawn_entry() -> Result<Option<alloc::sync::Arc<crate::basis::unit::task::Task>>, BootError> {
    let Some(region) = facts::info().initrd() else {
        return Ok(None);
    };
    let blob: &'static [u8] =
        unsafe { ::core::slice::from_raw_parts(region.base as *const u8, region.size) };
    let capsule =
        kernel_api::boot::initrd::entry_image(blob).ok_or(BootError::InvalidEntryImage {
            base: region.base,
            size: region.size,
        })?;
    let team = crate::basis::unit::team::capsule::assemble(capsule).map_err(|source| {
        BootError::Mapping {
            operation: MapOperation::AssembleCapsule,
            source,
        }
    })?;

    let mapping_size = region.size.next_multiple_of(PAGE_SIZE);
    let mapping = map_borrowed(&team.nest, PhysAddr::from_raw(region.base), mapping_size).map_err(
        |source| BootError::Mapping {
            operation: MapOperation::ImageView,
            source,
        },
    )?;

    let mut registry = crate::entry::boot::resources::Registry::default();
    resources::register_space(&mut registry, team.nest.clone()).map_err(|source| {
        BootError::Resources {
            operation: ResourceOperation::Pend,
            source,
        }
    })?;
    crate::entry::boot::devices::register(&mut registry).map_err(|source| {
        BootError::Resources {
            operation: ResourceOperation::Devices,
            source,
        }
    })?;
    crate::entry::trap::resources::register(&mut registry).map_err(|source| {
        BootError::Resources {
            operation: ResourceOperation::Traps,
            source,
        }
    })?;
    crate::entry::observe::register(&mut registry).map_err(|source| BootError::Resources {
        operation: ResourceOperation::Traps,
        source,
    })?;
    let resources = registry.freeze().map_err(|e| BootError::Resources {
        operation: ResourceOperation::Freeze,
        source: e.into_parts().0,
    })?;
    let ledger_len = crate::entry::boot::resources::ledger::size(
        resources.len()
            + crate::entry::boot::resources::CONTROL_ENTRIES
            + crate::entry::boot::resources::CALL_ENTRIES,
    )
    .map_err(|source| BootError::Resources {
        operation: ResourceOperation::LedgerSize,
        source,
    })?;
    let (ledger_pa, ledger_bytes) = crate::entry::boot::resources::ledger::block();
    let ledger = map_borrowed(&team.nest, PhysAddr::from_raw(ledger_pa), ledger_bytes).map_err(
        |source| BootError::Mapping {
            operation: MapOperation::LedgerView,
            source,
        },
    )?;

    let mut args = [0usize; kernel_api::boot::args::LEN];
    args[kernel_api::boot::args::VIEW] = mapping.as_usize();
    args[kernel_api::boot::args::VIEW_LEN] = region.size;
    args[kernel_api::boot::args::LEDGER] = ledger.as_usize();
    args[kernel_api::boot::args::LEDGER_LEN] = ledger_len;
    let mut words = alloc::vec::Vec::new();
    words
        .try_reserve_exact(args.len())
        .map_err(|_| BootError::Bootstrap(NestError::OutOfMemory))?;
    words.extend_from_slice(&args);
    let bootstrap = team
        .task()
        .args(words)
        .hold()
        .map_err(BootError::Bootstrap)?;
    let entries = resources
        .authority(&bootstrap)
        .map_err(|e| BootError::Resources {
            operation: ResourceOperation::Pend,
            source: e.into_parts().0,
        })?;
    // 可信控制者那一枚独立保留授权：只在这里铸、只发给这一个任务，
    // 与上面那批具名资源同走账本一条路。
    let mut entries = entries;
    entries
        .try_reserve(
            crate::entry::boot::resources::CONTROL_ENTRIES
                + crate::entry::boot::resources::CALL_ENTRIES,
        )
        .map_err(|_| BootError::Bootstrap(NestError::OutOfMemory))?;
    entries.extend(
        crate::entry::boot::resources::grant_controls(&bootstrap).map_err(|source| {
            BootError::Resources {
                operation: ResourceOperation::Pend,
                source,
            }
        })?,
    );
    entries.push(
        crate::entry::boot::resources::grant_calls(&bootstrap).map_err(|source| {
            BootError::Resources {
                operation: ResourceOperation::Pend,
                source,
            }
        })?,
    );
    crate::entry::boot::resources::ledger::write(&entries).map_err(|source| {
        BootError::Resources {
            operation: ResourceOperation::WriteLedger,
            source,
        }
    })?;
    crate::basis::unit::task::Task::release(&bootstrap).map_err(|source| {
        BootError::BootstrapRelease {
            task: bootstrap.ident.id,
            source,
        }
    })?;

    #[cfg(debug_assertions)]
    team.nest.audit();

    #[cfg(debug_assertions)]
    kernel().expect("kernel team not initialized").nest.audit();

    Ok(Some(bootstrap))
}

/// 把一段调用方仍保留的物理区间挂进 `Normal` 窗口的最低空档（只读）。
///
/// 借用凭证与来源在登记占用之前就已备齐：来源失败不留下无人持有的地址。
fn map_borrowed(nest: &Arc<Nest>, physical: PhysAddr, size: usize) -> Result<VirtAddr, NestError> {
    let borrowed = unsafe { BorrowedRegion::new(physical, size)? };
    let source = Flat::from_region(borrowed)?.whole();
    let reservation = nest.reserve(Placement::lowest(RegionKind::Normal, size, PAGE_SIZE)?)?;
    let mapping = nest.attach(reservation, source, Access::READ, Access::READ)?;
    Ok(mapping.initial_start())
}

pub(crate) fn boot_harts() -> Result<(), BootError> {
    let me = hart::hart_id();
    let count = hart::hart_count();
    for hart in 0..count {
        if HartId::new(hart) == me {
            continue;
        }
        let slot = HartId::new(hart);
        let opaque = hart::prepare_boot(slot, boot_stack(hart));
        let r = crate::metal::riscv::boot::start_hart(slot, opaque);
        r.map_err(|source| BootError::HartStart {
            hart: HartId::new(hart),
            source,
        })?;
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn boot_main() -> ! {
    let ktc = kernel()
        .expect("kernel team not initialized")
        .nest
        .physical_at(hart::hart_frame())
        .expect("kernel frame not mapped");
    let frame = unsafe { &*(ktc.as_usize() as *const TrapContext) };
    unsafe { crate::metal::riscv::boot::activate_secondary(frame) };
    arm_hart();
    identity::occupy(crate::basis::nest::Asid::kernel());
    run()
}
