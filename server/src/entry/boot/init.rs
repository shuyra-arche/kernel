use super::BootError;
use super::PciRoute;
use super::ResourceOperation;
use crate::basis::room::interrupt;
use crate::machine::board;
use crate::machine::clock;
use crate::machine::console;

pub fn init(dtp: usize) -> Result<(), BootError> {
    initialize(dtp, None)
}

/// Install a platform-selected PCI route before exporting boot resources.
///
/// # Safety
/// The platform owns the endpoint, its BAR and the controller exclusively.
/// All requests traverse the IOMMU, with no bypass or requester alias. The
/// quiescence callback runs in bounded steps after the IOMMU is off. Settled
/// must prove that all earlier endpoint accesses that could reach old Pages
/// have completed and that future accesses remain blocked.
pub unsafe fn init_with_pci(dtp: usize, route: PciRoute) -> Result<(), BootError> {
    initialize(dtp, Some(route))
}

fn initialize(dtp: usize, route: Option<PciRoute>) -> Result<(), BootError> {
    console::init();
    board::init(dtp).map_err(|source| BootError::Machine { dtp, source })?;
    crate::machine::allocator::init().map_err(BootError::Allocator)?;
    // 锁序检测要在任何被验证的调用之前启用：分配器就绪即可建池，
    // 之后的 mode / 空间 / 时钟 / 栈初始化都在它的看护下跑。
    // 池按机器建一次：测试台架会重复走公共 init，重复就是“已经启用”。
    #[cfg(debug_assertions)]
    match crate::machine::sync::init_depend(crate::machine::hart::hart_count()) {
        Ok(()) | Err(crate::machine::sync::DepInitError::AlreadyInit) => {}
        Err(error) => return Err(BootError::Dependencies(error)),
    }
    crate::machine::mmu::riscv::mode::detect().map_err(BootError::PagingMode)?;
    super::space::init().map_err(BootError::KernelSpace)?;
    clock::init().map_err(BootError::Clock)?;
    super::devices::init_iommu().map_err(|source| BootError::Resources {
        operation: ResourceOperation::InitializeIommu,
        source,
    })?;
    if let Some(route) = route {
        super::devices::init_pci(route).map_err(|source| BootError::Resources {
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
