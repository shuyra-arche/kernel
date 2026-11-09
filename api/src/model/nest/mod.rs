//! Stable nest identity. A numeric identity is descriptive and never grants access.
use crate::abi::register::{Decode, Wire};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct NestId(usize);
impl NestId {
    pub const fn new(value: usize) -> Option<Self> {
        if value > 0 && value <= isize::MAX as usize {
            Some(Self(value))
        } else {
            None
        }
    }
    pub const fn get(self) -> usize {
        self.0
    }
}
impl Wire for NestId {
    const WORDS: usize = 1;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        self.0.pack(words, at);
    }
    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, Decode> {
        Self::new(usize::unpack(words, at)?).ok_or(Decode::Invalid)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum NestKind {
    Internal = 0,
    External = 1,
}
impl Wire for NestKind {
    const WORDS: usize = 1;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        (*self as usize).pack(words, at);
    }
    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, Decode> {
        match usize::unpack(words, at)? {
            0 => Ok(Self::Internal),
            1 => Ok(Self::External),
            _ => Err(Decode::Invalid),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum NestPhase {
    Unbound = 0,
    Bound = 1,
    Closing = 2,
    Closed = 3,
    Failed = 4,
}
/// Retained context observation. Binding identity is the never-reused NestId;
/// a closed external Nest cannot be rebound. A new Nest may acquire the settled Port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NestInfo {
    pub id: NestId,
    pub kind: NestKind,
    pub phase: NestPhase,
    pub port: usize,
    pub fault_sequence: u64,
    pub fault_cause: u64,
    pub fault_address: u64,
    pub fault_requester: u64,
}
impl NestInfo {
    pub const WIDTH: usize = 64;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        crate::abi::layout::store_words(
            [
                self.id.get(),
                self.kind as usize,
                self.phase as usize,
                self.port,
                self.fault_sequence as usize,
                self.fault_cause as usize,
                self.fault_address as usize,
                self.fault_requester as usize,
            ],
            &mut bytes,
        );
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let w: [usize; 8] = crate::abi::layout::fetch_words(bytes)?;
        Some(Self {
            id: NestId::new(w[0])?,
            kind: match w[1] {
                0 => NestKind::Internal,
                1 => NestKind::External,
                _ => return None,
            },
            phase: match w[2] {
                0 => NestPhase::Unbound,
                1 => NestPhase::Bound,
                2 => NestPhase::Closing,
                3 => NestPhase::Closed,
                4 => NestPhase::Failed,
                _ => return None,
            },
            port: w[3],
            fault_sequence: w[4] as u64,
            fault_cause: w[5] as u64,
            fault_address: w[6] as u64,
            fault_requester: w[7] as u64,
        })
    }
}
