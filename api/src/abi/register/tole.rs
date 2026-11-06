use super::{Decode, Wire};
use crate::model::tole::ToleEventSource;

impl Wire for ToleEventSource {
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
