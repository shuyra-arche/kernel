//! RISC-V supervisor firmware calls used by the Kernel.
//! Board discovery and routing remain with the external platform owner.
pub mod ecall;

pub trait Extension {
    const EID: usize;
    type Fid: Into<usize> + Copy;
}

mod timer;
pub use timer::{Timer, TimerCall};
mod ipi;
pub use ipi::{Ipi, IpiCall};
mod fence;
pub use fence::{Rfence, RfenceCall};
mod hart;
pub use hart::{Hsm, HsmCall};
mod reset;
pub use reset::{SystemReset, SystemResetCall};
mod console;
pub use console::{Dbcn, DbcnCall};
