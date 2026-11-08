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
