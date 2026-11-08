//! Timer firmware extension. Function numbers are part of the SBI wire contract.
use super::{Extension, ecall::ScallBuilder};

#[repr(usize)]
#[derive(Clone, Copy)]
pub enum Timer {
    SetTimer = 0,
}

impl From<Timer> for usize {
    fn from(value: Timer) -> Self {
        value as usize
    }
}

pub struct TimerExt;
impl Extension for TimerExt {
    const EID: usize = 0x54494d45;
    type Fid = Timer;
}

pub type TimerCall = ScallBuilder<TimerExt>;
