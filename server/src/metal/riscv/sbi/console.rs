//! Dbcn firmware extension. Function numbers are part of the SBI wire contract.
use super::{Extension, ecall::ScallBuilder};

#[repr(usize)]
#[derive(Clone, Copy)]
pub enum Dbcn {
    ConsoleWrite = 0,
    ConsoleRead = 1,
    ConsoleWriteByte = 2,
}

impl From<Dbcn> for usize {
    fn from(value: Dbcn) -> Self {
        value as usize
    }
}

pub struct DbcnExt;
impl Extension for DbcnExt {
    const EID: usize = 0x4442434e;
    type Fid = Dbcn;
}

pub type DbcnCall = ScallBuilder<DbcnExt>;
