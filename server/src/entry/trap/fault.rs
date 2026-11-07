use riscv::register::scause;
use riscv::register::sepc;
use riscv::register::stval;

use crate::basis::nest::FaultKind;
use kernel_api::address::VirtAddr;

#[derive(Debug)]
pub struct PageFault {
    pub addr: VirtAddr,
    pub pc: usize,
    pub kind: FaultKind,
}

impl PageFault {
    pub unsafe fn capture() -> Self {
        let code = scause::read().code();
        let kind = match code {
            12 => FaultKind::Instruction,
            13 => FaultKind::Load,
            15 => FaultKind::Store,
            _ => panic!("capture() called on non-page-fault scause={}", code),
        };

        Self {
            addr: crate::machine::mmu::riscv::canonical(stval::read()),
            pc: sepc::read(),
            kind,
        }
    }
}
