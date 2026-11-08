use crate::abi::register::{Decode, Wire};
use crate::abi::reply::FromPair;

/// A Sign handle is resolved only in the calling Task's table.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SignId(usize);
impl SignId {
    pub const fn new(raw: usize) -> Self {
        Self(raw)
    }
    pub const fn get(self) -> usize {
        self.0
    }
}

impl Wire for SignId {
    const WORDS: usize = 1;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        self.get().pack(words, at);
    }
    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, Decode> {
        let raw = usize::unpack(words, at)?;
        if raw == 0 {
            return Err(Decode::Invalid);
        }
        Ok(Self::new(raw))
    }
}

impl FromPair for SignId {
    fn from_pair(raw: usize, _: usize) -> Self {
        Self::new(raw)
    }
}

bitflags::bitflags! {
    /// Independent control-flow authority; no Page access or Unit control follows.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct SignRights: u8 {
        const LISTEN = 1;
        const TRIGGER = 2;
        const ROUTE = 4;
    }
}

impl Wire for SignRights {
    const WORDS: usize = 1;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        (self.bits() as usize).pack(words, at);
    }
    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, Decode> {
        let bits = u8::try_from(usize::unpack(words, at)?).map_err(|_| Decode::Invalid)?;
        Self::from_bits(bits)
            .filter(|rights| !rights.is_empty())
            .ok_or(Decode::Invalid)
    }
}
