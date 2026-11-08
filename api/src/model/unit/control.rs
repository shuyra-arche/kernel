use crate::abi::register::{Decode, Wire};
use crate::abi::reply::FromPair;

/// Task-local administrative qualification; never a memory borrow or event right.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlId(usize);
impl ControlId {
    pub const NONE: Self = Self(0);
    pub const fn new(raw: usize) -> Self {
        Self(raw)
    }
    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
    Build,
    Guard,
}

impl Wire for ControlId {
    const WORDS: usize = 1;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        self.get().pack(words, at);
    }
    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, Decode> {
        usize::unpack(words, at).map(Self::new)
    }
}

impl FromPair for ControlId {
    fn from_pair(raw: usize, _: usize) -> Self {
        Self::new(raw)
    }
}
