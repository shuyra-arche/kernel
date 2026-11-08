//! Ipi firmware extension. Function numbers are part of the SBI wire contract.
use super::{Extension, ecall::ScallBuilder};

#[repr(usize)]
#[derive(Clone, Copy)]
pub enum Ipi {
    SendIpi = 0,
}

impl From<Ipi> for usize {
    fn from(value: Ipi) -> Self {
        value as usize
    }
}

pub struct IpiExt;
impl Extension for IpiExt {
    const EID: usize = 0x735049;
    type Fid = Ipi;
}

pub type IpiCall = ScallBuilder<IpiExt>;
