use super::{Decode, Wire};
use crate::model::pend::PendRange;

impl Wire for PendRange {
    const WORDS: usize = PendRange::WORDS;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        match *self {
            Self::Inherit => {
                0usize.pack(s, i);
                0usize.pack(s, i);
                0usize.pack(s, i);
            }
            Self::Pages { offset, count } => {
                1usize.pack(s, i);
                offset.pack(s, i);
                count.pack(s, i);
            }
        }
    }

    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let tag = usize::unpack(s, i)?;
        let offset = usize::unpack(s, i)?;
        let count = usize::unpack(s, i)?;
        match tag {
            0 if offset == 0 && count == 0 => Ok(Self::Inherit),
            1 => Self::pages(offset, count).ok_or(Decode::Invalid),
            _ => Err(Decode::Invalid),
        }
    }
}
