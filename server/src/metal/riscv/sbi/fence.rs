//! Rfence firmware extension. Function numbers are part of the SBI wire contract.
use super::{Extension, ecall::ScallBuilder};

#[repr(usize)]
#[derive(Clone, Copy)]
pub enum Rfence {
    RemoteFenceI = 0,
    RemoteSfenceVma = 1,
    RemoteSfenceVmaAsid = 2,
}

impl From<Rfence> for usize {
    fn from(value: Rfence) -> Self {
        value as usize
    }
}

pub struct RfenceExt;
impl Extension for RfenceExt {
    const EID: usize = 0x52464e43;
    type Fid = Rfence;
}

pub type RfenceCall = ScallBuilder<RfenceExt>;
