use super::BootError;
use super::ResourceOperation;
use crate::basis::room::interrupt;
use crate::machine::board;
use crate::machine::clock;
use crate::machine::console;

pub fn init(dtp: usize) -> Result<(), BootError> {
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
    super::unit::init().map_err(BootError::KernelSpace)?;
    clock::init().map_err(BootError::Clock)?;
    super::iommu::init().map_err(|source| BootError::Resources {
        operation: ResourceOperation::InitializeIommu,
        source,
    })?;
    super::pci::init().map_err(|source| BootError::Resources {
        operation: ResourceOperation::Devices,
        source,
    })?;
    interrupt::init().map_err(|source| BootError::Resources {
        operation: ResourceOperation::InitializeTraps,
        source,
    })?;
    super::stack::init().map_err(BootError::TrapStacks)?;
    Ok(())
}
