//! A snapshot of a Cell position and its publication state.

use crate::abi::layout::{fetch_words, store_words};
use crate::address::VirtAddr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum CellState {
    Vacant = 0,
    Ready = 1,
    Published = 2,
    Publishing = 3,
    Retracting = 4,
    Failed = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellInfo {
    pub generation: u64,
    pub state: CellState,
    pub page: u64,
    pub frozen: bool,
    pub external: bool,
    pub address: VirtAddr,
    pub fault_cause: u64,
    pub fault_address: u64,
}

impl CellInfo {
    pub const WIDTH: usize = 64;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut out = [0; Self::WIDTH];
        store_words(
            [
                self.generation as usize,
                self.state as usize,
                self.page as usize,
                self.frozen as usize | ((self.external as usize) << 1),
                self.address.get(),
                self.fault_cause as usize,
                self.fault_address as usize,
                0,
            ],
            &mut out,
        );
        out
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let [
            generation,
            state,
            page,
            flags,
            address,
            fault_cause,
            fault_address,
            reserved,
        ] = fetch_words(bytes)?;
        if flags > 3 || reserved != 0 {
            return None;
        }
        Some(Self {
            generation: generation as u64,
            state: match state {
                0 => CellState::Vacant,
                1 => CellState::Ready,
                2 => CellState::Published,
                3 => CellState::Publishing,
                4 => CellState::Retracting,
                5 => CellState::Failed,
                _ => return None,
            },
            page: page as u64,
            frozen: flags & 1 != 0,
            external: flags & 2 != 0,
            address: VirtAddr::new(address),
            fault_cause: fault_cause as u64,
            fault_address: fault_address as u64,
        })
    }
}
