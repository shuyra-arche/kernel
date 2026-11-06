use crate::model::pole::{Bit, PoleCondition};

impl crate::abi::register::Wire for PoleCondition {
    const WORDS: usize = 1;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        let Self::Signal(bit) = self;
        (3 + bit.index()).pack(words, at);
    }

    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, crate::abi::register::Decode> {
        let raw = usize::unpack(words, at)?;
        raw.checked_sub(3)
            .and_then(Bit::of)
            .map(Self::Signal)
            .ok_or(crate::abi::register::Decode::Invalid)
    }
}

impl crate::abi::register::Wire for Bit {
    const WORDS: usize = 1;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        self.index().pack(words, at);
    }
    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, crate::abi::register::Decode> {
        Self::of(usize::unpack(words, at)?).ok_or(crate::abi::register::Decode::Invalid)
    }
}
