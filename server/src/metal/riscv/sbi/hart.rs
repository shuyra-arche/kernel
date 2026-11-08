//! Hsm firmware extension. Function numbers are part of the SBI wire contract.
use super::{Extension, ecall::ScallBuilder};

#[repr(usize)]
#[derive(Clone, Copy)]
pub enum Hsm {
    Start = 0,
    Stop = 1,
}

impl From<Hsm> for usize {
    fn from(value: Hsm) -> Self {
        value as usize
    }
}

pub struct HsmExt;
impl Extension for HsmExt {
    const EID: usize = 0x48534d;
    type Fid = Hsm;
}

pub type HsmCall = ScallBuilder<HsmExt>;
