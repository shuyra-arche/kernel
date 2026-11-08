//! Monotonic deadlines use the same nanosecond epoch as Room.Clock.
use crate::abi::register::{Decode, Wire};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wait {
    Poll,
    At(u64),
    Infinite,
}

impl Wire for Wait {
    const WORDS: usize = 2;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        let (tag, at): (usize, u64) = match self {
            Self::Poll => (0, 0),
            Self::At(at) => (1, *at),
            Self::Infinite => (2, 0),
        };
        tag.pack(s, i);
        (at as usize).pack(s, i);
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let tag = usize::unpack(s, i)?;
        let at = usize::unpack(s, i)?;
        match (tag, at) {
            (0, 0) => Ok(Self::Poll),
            (1, at) => Ok(Self::At(at as u64)),
            (2, 0) => Ok(Self::Infinite),
            _ => Err(Decode::Invalid),
        }
    }
}
