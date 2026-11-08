use super::BootError;
use super::ResourceOperation;
use super::{ExternalRoute, MapMmio};
use crate::basis::room::interrupt;
use crate::metal::board;
use crate::metal::clock;
use crate::metal::console;
use kernel_api::abi::call::pend::PendFail;

pub fn init(dtp: usize) -> Result<(), BootError> {
    initialize(dtp, None)
}

/// Prepare an external route using trusted code after Kernel MMIO and IOMMU setup.
/// No route is published to Units until preparation and validation both succeed.
///
/// # Safety
/// The platform exclusively owns the requester, resource window and controller.
/// All requests traverse the selected IOMMU without bypass, ATS/PRI or mutable
/// routing. The provider satisfies Quiescence for every requester of this
/// one-shot controller. It may enable the endpoint while DDTP is Off: Kernel
/// never uses device business controls as a prerequisite for withdrawal.
/// The supplied mapper may only be used for exclusively owned MMIO; mappings
/// remain valid for the boot lifetime. Preparation failure must leave access
/// blocked by the IOMMU and must not install an independently usable route.
/// Physical hardware needs its own verified ordering/coherence contract.
/// This is a trusted Rust boot interface, not a syscall or driver callback.
pub unsafe fn init_with_external(
    dtp: usize,
    prepare: fn(usize, MapMmio) -> Result<ExternalRoute, PendFail>,
) -> Result<(), BootError> {
    initialize(dtp, Some(prepare))
}

fn initialize(
    dtp: usize,
    prepare: Option<fn(usize, MapMmio) -> Result<ExternalRoute, PendFail>>,
) -> Result<(), BootError> {
    console::init();
    board::init(dtp).map_err(|source| BootError::Machine { dtp, source })?;
    crate::metal::mmu::riscv::mode::init().map_err(BootError::PagingMode)?;
    console::_write(format_args!(
        "cpu: admitted={} boot={} paging={:?}\n",
        board::info().hart.count,
        crate::metal::hart::firmware_id().get(),
        crate::metal::mmu::riscv::mode::mode(),
    ));
    crate::metal::allocator::init().map_err(BootError::Allocator)?;
    // 锁序检测要在任何被验证的调用之前启用：分配器就绪即可建池，
    // 之后的空间 / 时钟 / 栈初始化都在它的看护下跑。
    // 池按机器建一次：测试台架会重复走公共 init，重复就是“已经启用”。
    #[cfg(debug_assertions)]
    match crate::metal::sync::init_depend(crate::metal::hart::hart_count()) {
        Ok(()) | Err(crate::metal::sync::DepInitError::AlreadyInit) => {}
        Err(error) => return Err(BootError::Dependencies(error)),
    }
    super::space::init().map_err(BootError::KernelSpace)?;
    clock::init().map_err(BootError::Clock)?;
    super::devices::init_iommu().map_err(|source| BootError::Resources {
        operation: ResourceOperation::InitializeIommu,
        source,
    })?;
    if let Some(prepare) = prepare {
        let route = prepare(dtp, super::devices::external::mapped).map_err(|source| {
            BootError::Resources {
                operation: ResourceOperation::Devices,
                source,
            }
        })?;
        super::devices::init_external(route).map_err(|source| BootError::Resources {
            operation: ResourceOperation::Devices,
            source,
        })?;
    }
    interrupt::init().map_err(|source| BootError::Resources {
        operation: ResourceOperation::InitializeTraps,
        source,
    })?;
    super::space::init_stacks().map_err(BootError::TrapStacks)?;
    Ok(())
}
