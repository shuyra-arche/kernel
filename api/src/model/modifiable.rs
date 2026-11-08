//! Pend's only access choice. Possession grants reading; modification requires Yes.
//! Delegation never adds access, and borrowing rules are enforced by Kernel state.
use crate::abi::register::{Decode, Wire};

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Modifiable {
    No = 0,
    Yes = 1,
}
impl Modifiable {
    pub const fn bits(self) -> u32 {
        self as u32
    }
    pub const fn from_bits(bits: u32) -> Option<Self> {
        match bits {
            0 => Some(Self::No),
            1 => Some(Self::Yes),
            _ => None,
        }
    }
    pub const fn allows(self, requested: Self) -> bool {
        matches!(self, Self::Yes) || matches!(requested, Self::No)
    }
}

impl Wire for Modifiable {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        s[*i] = self.bits() as usize;
        *i += 1;
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let v = *s.get(*i).ok_or(Decode::Overflow)?;
        *i += 1;
        let bits = u32::try_from(v).map_err(|_| Decode::Invalid)?;
        Modifiable::from_bits(bits).ok_or(Decode::Invalid)
    }
}
