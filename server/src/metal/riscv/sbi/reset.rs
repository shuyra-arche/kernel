//! SystemReset firmware extension. Function numbers are part of the SBI wire contract.
use super::{Extension, ecall::ScallBuilder};

#[repr(usize)]
#[derive(Clone, Copy)]
pub enum SystemReset {
    SystemReset = 0,
}

impl From<SystemReset> for usize {
    fn from(value: SystemReset) -> Self {
        value as usize
    }
}

pub struct SystemResetExt;
impl Extension for SystemResetExt {
    const EID: usize = 0x53525354;
    type Fid = SystemReset;
}

pub type SystemResetCall = ScallBuilder<SystemResetExt>;
