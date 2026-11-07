use super::{Decode, Wire};
use crate::model::sign::SignSource;

impl Wire for SignSource {
    const WORDS: usize = 3;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        for word in self.words() {
            word.pack(words, at);
        }
    }

    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, Decode> {
        let mut source = [0; 3];
        for word in &mut source {
            *word = usize::unpack(words, at)?;
        }
        Self::of(source).ok_or(Decode::Invalid)
    }
}

impl Wire for crate::model::sign::SignId {
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
impl Wire for crate::model::sign::SignRights {
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
