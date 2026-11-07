use super::{Decode, Wire};
use crate::model::clone_view::{CloneCondition, CloneLimits};

impl Wire for CloneLimits {
    const WORDS: usize = 3;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        self.max_len.pack(words, at);
        self.max_messages.pack(words, at);
        self.max_bytes.pack(words, at);
    }

    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, Decode> {
        let limits = Self {
            max_len: usize::unpack(words, at)?,
            max_messages: usize::unpack(words, at)?,
            max_bytes: usize::unpack(words, at)?,
        };
        if limits.valid() {
            Ok(limits)
        } else {
            Err(Decode::Invalid)
        }
    }
}

impl crate::abi::register::Wire for CloneCondition {
    const WORDS: usize = 1;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        (*self as usize).pack(words, at);
    }

    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, crate::abi::register::Decode> {
        match usize::unpack(words, at)? {
            0 => Ok(Self::Pull),
            1 => Ok(Self::Push),
            2 => Ok(Self::Empty),
            _ => Err(crate::abi::register::Decode::Invalid),
        }
    }
}
