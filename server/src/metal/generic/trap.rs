//! Normalized trap events. The provider supplies raw fault diagnostics for reporting.

use fack::prelude::Error;
use kernel_api::address::VirtAddr;

pub(crate) enum TrapEvent {
    Timer,
    Ipi,
    External,
    Call,
    PageFault(PageFault),
    IllegalInstruction(Fault),
    Exception(Fault),
}

pub(crate) struct Fault {
    pub(crate) cause: usize,
    pub(crate) value: usize,
    pub(crate) pc: usize,
}

pub(crate) struct PageFault {
    pub(crate) detail: Fault,
    pub(crate) addr: VirtAddr,
    pub(crate) kind: FaultKind,
}

#[derive(Error, Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum FaultKind {
    #[error("Execute OOM Instruction")]
    Instruction,
    #[error("Load OOM Data")]
    Load,
    #[error("Store OOM Data")]
    Store,
}
