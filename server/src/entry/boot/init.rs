use super::BootError;
use super::ExternalRoute;
use super::ResourceOperation;
use crate::basis::room::interrupt;
use crate::entry::boot::facts;
use crate::metal::clock;
use crate::metal::console;
use kernel_api::abi::call::pend::PendFail;

/// Initialize allocation and CPU translation from immutable admitted facts.
/// # Safety
/// Descriptions reflect the actual machine. Firmware has granted exclusive
/// ownership of free RAM and the executing image; referenced storage is immutable.
pub unsafe fn init(platform: crate::metal::platform::Platform) -> Result<(), BootError> {
    console::init();
    facts::init(platform)?;
    crate::metal::mmu::mode::init().map_err(BootError::PagingMode)?;
    console::_write(format_args!(
        "cpu: admitted={} boot={} paging={:?}\n",
        facts::info().hart.count(),
        crate::metal::hart::firmware_id().get(),
        crate::metal::mmu::mode::mode(),
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
    Ok(())
}

/// Configure protected controls before publishing any Unit authority.
/// # Safety
/// Descriptions reflect actual immutable routes and exclusive MMIO custody.
/// Queue RAM and register accesses satisfy the selected backend's coherence and
/// ordering contract. The caller excludes bypass, ATS/PRI and mutable routing.
pub unsafe fn configure(
    description: crate::metal::platform::devices::Devices<'_>,
) -> Result<(), BootError> {
    super::devices::admit(description).map_err(|source| BootError::Resources {
        operation: ResourceOperation::Devices,
        source,
    })?;
    super::devices::init_iommu(description).map_err(|source| BootError::Resources {
        operation: ResourceOperation::InitializeIommu,
        source,
    })?;
    interrupt::init().map_err(|source| BootError::Resources {
        operation: ResourceOperation::InitializeTraps,
        source,
    })?;
    super::space::init_stacks().map_err(BootError::TrapStacks)?;
    super::devices::configured();
    Ok(())
}

/// Bind an exclusively held resource to its admitted translation controller.
/// # Safety
/// The platform owns this source and its complete isolation group. All its
/// requests traverse that controller without bypass/ATS/PRI. Quiescence covers
/// every requester and retains the platform evidence for the entire binding.
pub unsafe fn bind_external(route: ExternalRoute) -> Result<(), BootError> {
    super::devices::init_external(route).map_err(|source| BootError::Resources {
        operation: ResourceOperation::Devices,
        source,
    })
}
/// Map exclusively held MMIO for trusted platform preparation.
/// # Safety
/// Physical interval is MMIO owned by this image, never allocator-owned RAM.
pub unsafe fn map_mmio(base: usize, size: usize) -> Result<usize, PendFail> {
    super::devices::external::mapped(base, size)
}

/// Grant a validated external MMIO interval to the initial Unit.
/// # Safety
/// The interval is independently assignable raw device memory. The trusted
/// manager must revoke former driver access before reassigning an external source.
pub unsafe fn grant_mmio(region: kernel_api::boot::platform::Region) -> Result<(), BootError> {
    super::devices::external::grant(region).map_err(|source| BootError::Resources {
        operation: ResourceOperation::Devices,
        source,
    })
}

/// Grant one independently owned, level-high PLIC source to the initial Unit.
/// # Safety
/// The external platform validated the complete DT route, exclusive ownership,
/// and level-high gateway wiring. Shared/cascaded/MSI routes are not supported.
/// Call only during single-Hart boot preparation, before `start` publishes grants.
pub unsafe fn grant_interrupt(controller: usize, source: u32) -> Result<(), BootError> {
    crate::basis::room::interrupt::register(controller, source).map_err(|source| {
        BootError::Resources {
            operation: ResourceOperation::Devices,
            source,
        }
    })
}
