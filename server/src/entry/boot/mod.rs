pub(crate) mod devices;
pub(crate) mod error;
mod init;
pub(crate) mod initrd;
mod iommu;
mod pci;
pub(crate) mod resources;
mod stack;
pub(crate) mod unit;

pub use error::BootError;
pub use error::MapOperation;
pub use error::ResourceOperation;
pub use error::fail;
pub use init::init;

use ::core::arch::global_asm;

use alloc::sync::Arc;

use riscv::register::satp;

use crate::basis::flat::BorrowedRegion;
use crate::basis::flat::Flat;
use crate::basis::flat::FlatRange;
use crate::basis::nest::Access;
use crate::basis::nest::ActiveRange;
use crate::basis::nest::Nest;
use crate::basis::nest::NestError;
use crate::basis::nest::Placement;
use crate::basis::nest::RegionId;
use crate::basis::room::conductor;
use crate::basis::room::scheduler;
use crate::basis::unit::team::kernel;
use crate::machine::board;
use crate::machine::hart;
use crate::machine::hart::HartId;
use crate::machine::image::ROOT_STACK_CANARY;
use crate::machine::image::root_stack_base;
use crate::machine::mmu::riscv::identity;
use crate::machine::mmu::riscv::mode;
use crate::machine::mmu::riscv::table::PteFlags;
use crate::machine::switcher::context::TrapContext;
use crate::machine::switcher::stack::arm_hart;
use crate::machine::switcher::stack::boot_stack;
use crate::machine::switcher::trampoline::restore;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PhysAddr;
use kernel_api::address::VirtAddr;

global_asm!(
    ".section .text.boot",
    ".align 2",
    ".globl _boot_entry",
    "_boot_entry:",
    "    la   t0, PER_HART",
    "    slli t1, a0, 6",
    "    add  tp, t0, t1",
    "    csrc sstatus, 2",
    "    mv   sp, a1",
    "    call boot_main",
);

unsafe extern "C" {
    static _boot_entry: u8;
}

pub fn start() -> Result<(), BootError> {
    scheduler::init().map_err(BootError::Scheduler)?;
    register_runtime_hooks();

    if let Some(entry) = spawn_entry()? {
        crate::basis::room::halt::beacon::arm(&entry);
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
    let top = crate::machine::switcher::stack::trap_stack_edge(hart::hart_id()).get();
    unsafe {
        ::core::arch::asm!(
            "mv sp, {top}",
            "j {enter}",
            top = in(reg) top,
            enter = sym run_on_stack,
            options(noreturn),
        );
    }
}

extern "C" fn run_on_stack() -> ! {
    restore(conductor::run::run())
}

pub(crate) fn register_runtime_hooks() {
    use crate::basis::room::halt;

    static EXIT_HOOKS: &[fn(&alloc::sync::Arc<crate::basis::unit::task::Task>)] = &[
        crate::basis::unit::lifecycle::doom,
        crate::basis::pend::doom,
    ];
    crate::basis::unit::lifecycle::hook(EXIT_HOOKS);

    const SHUTDOWN_HOOKS: &[fn()] = &[
        crate::entry::boot::iommu::shutdown,
        crate::entry::report::shutdown::report,
        crate::basis::room::scheduler::rip,
        crate::machine::allocator::block::flush,
    ];
    halt::hook(SHUTDOWN_HOOKS, crate::entry::report::shutdown::waiting);
}

fn spawn_entry() -> Result<Option<alloc::sync::Arc<crate::basis::unit::task::Task>>, BootError> {
    let Some(region) = board::info().initrd() else {
        return Ok(None);
    };
    let blob: &'static [u8] =
        unsafe { ::core::slice::from_raw_parts(region.base as *const u8, region.size) };
    let capsule =
        crate::entry::boot::initrd::entry_image(blob).ok_or(BootError::InvalidEntryImage {
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
    let mapping = map_borrowed(
        &team.space.nest,
        PhysAddr::from_raw(region.base),
        mapping_size,
    )
    .map_err(|source| BootError::Mapping {
        operation: MapOperation::ImageView,
        source,
    })?;

    let mut registry = crate::entry::boot::resources::Registry::default();
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
    let resources = registry.freeze().map_err(|e| BootError::Resources {
        operation: ResourceOperation::Freeze,
        source: e.into_parts().0,
    })?;
    let ledger_len = crate::entry::boot::resources::ledger::size(
        resources.len()
            + crate::entry::boot::resources::CONTROL_ENTRIES
            + crate::entry::boot::resources::SIGN_ENTRIES,
    )
    .map_err(|source| BootError::Resources {
        operation: ResourceOperation::LedgerSize,
        source,
    })?;
    let (ledger_pa, ledger_bytes) = crate::entry::boot::resources::ledger::block();
    let ledger = map_borrowed(
        &team.space.nest,
        PhysAddr::from_raw(ledger_pa),
        ledger_bytes,
    )
    .map_err(|source| BootError::Mapping {
        operation: MapOperation::LedgerView,
        source,
    })?;

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
                + crate::entry::boot::resources::SIGN_ENTRIES,
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
        crate::entry::boot::resources::grant_sign(&bootstrap).map_err(|source| {
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
    team.space.nest.audit();

    #[cfg(debug_assertions)]
    kernel()
        .expect("kernel team not initialized")
        .space
        .nest
        .audit();

    Ok(Some(bootstrap))
}

/// 把一段调用方仍保留的物理区间挂进 `Normal` 窗口的最低空档（只读）。
///
/// 借用凭证与来源在登记占用之前就已备齐：来源失败不留下无人持有的地址。
fn map_borrowed(nest: &Arc<Nest>, physical: PhysAddr, size: usize) -> Result<VirtAddr, NestError> {
    let borrowed = unsafe { BorrowedRegion::new(physical, size)? };
    let source = Flat::from_region(borrowed)?.whole();
    let reservation = nest.reserve(Placement::lowest(RegionId::Normal, size, PAGE_SIZE)?)?;
    let mapping = nest.attach(reservation, source, Access::READ, Access::READ)?;
    Ok(mapping.initial_start())
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
    attrs: PteFlags,
) -> Result<(), NestError> {
    let size = source
        .page_count()
        .checked_bytes()
        .ok_or(NestError::NotAligned)?;
    let active = ActiveRange::new(start, size, access, access)?;
    let reservation = nest.reserve(Placement::exact(RegionId::Static, start, size)?)?;
    nest.attach_shaped(reservation, source, attrs, &[active])?;
    Ok(())
}

pub(crate) fn boot_harts() -> Result<(), BootError> {
    let me = hart::hart_id();
    let count = hart::hart_count();
    let entry = ::core::ptr::addr_of!(_boot_entry) as usize;
    for hart in 0..count {
        if HartId::new(hart) == me {
            continue;
        }
        let stack_top = boot_stack(hart);
        let r = sbi::HsmCall::new(sbi::fid::Hsm::Start)
            .args(sbi::ecall::SArgs {
                a0: hart,
                a1: entry,
                a2: stack_top,
                ..Default::default()
            })
            .call();
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
        .space
        .nest
        .physical_at(hart::hart_frame())
        .expect("kernel frame not mapped");
    let frame = unsafe { &*(ktc.as_usize() as *const TrapContext) };
    let ksatp = frame.kernel_satp;
    unsafe {
        hart::mark_online();
        satp::set(mode::mode(), ksatp.asid(), ksatp.ppn());
        ::core::arch::asm!("sfence.vma", "fence.i");
    }
    arm_hart();
    identity::occupy(crate::basis::nest::Asid::kernel());
    run()
}
