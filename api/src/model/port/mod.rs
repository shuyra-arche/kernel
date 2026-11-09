//! Stable port identity. A numeric identity is descriptive and never grants access.
use crate::abi::register::{Decode, Wire};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PortId(usize);
impl PortId {
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
impl Wire for PortId {
    const WORDS: usize = 1;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        self.0.pack(words, at);
    }
    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, Decode> {
        Self::new(usize::unpack(words, at)?).ok_or(Decode::Invalid)
    }
}

/// Descriptive platform facts, never credentials. The initial backend admits
/// one requester per controller, 4 KiB leaves in one aligned 2 MiB window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortInfo {
    pub id: PortId,
    pub controller: usize,
    pub requester: u32,
    pub address_bits: usize,
    pub page_size: usize,
    pub window_size: usize,
}
impl PortInfo {
    pub const WIDTH: usize = 48;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        crate::abi::layout::store_words(
            [
                self.id.get(),
                self.controller,
                self.requester as usize,
                self.address_bits,
                self.page_size,
                self.window_size,
            ],
            &mut bytes,
        );
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let w: [usize; 6] = crate::abi::layout::fetch_words(bytes)?;
        if w[3] != 38 || w[4] != 4096 || w[5] != 1 << 21 {
            return None;
        }
        Some(Self {
            id: PortId::new(w[0])?,
            controller: w[1],
            requester: u32::try_from(w[2]).ok()?,
            address_bits: w[3],
            page_size: w[4],
            window_size: w[5],
        })
    }
}
