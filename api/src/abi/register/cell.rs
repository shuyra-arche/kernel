use super::Decode;
use super::Wire;
use crate::model::cell::Allocation;
use crate::model::cell::MappingControl;
use crate::model::cell::MappingId;
use crate::model::cell::ReferenceRange;
use crate::model::cell::SlotId;
use crate::model::identity::{PendId, TeamId};

impl Wire for Allocation {
    const WORDS: usize = Allocation::WORDS;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        self.size.pack(s, i);
        self.align.pack(s, i);
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let size = usize::unpack(s, i)?;
        let align = usize::unpack(s, i)?;
        Self::new(size, align).ok_or(Decode::Invalid)
    }
}

impl Wire for ReferenceRange {
    const WORDS: usize = ReferenceRange::WORDS;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        self.reference.pack(s, i);
        self.offset.pack(s, i);
        self.count.pack(s, i);
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let reference = SlotId::unpack(s, i)?;
        let offset = usize::unpack(s, i)?;
        let count = usize::unpack(s, i)?;
        Self::new(reference, offset, count).ok_or(Decode::Invalid)
    }
}

impl Wire for MappingControl {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        let word = match self {
            Self::Caller => 0usize,
            Self::SpaceMembers => 1usize,
        };
        word.pack(s, i);
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        match usize::unpack(s, i)? {
            0 => Ok(Self::Caller),
            1 => Ok(Self::SpaceMembers),
            _ => Err(Decode::Invalid),
        }
    }
}

impl Wire for SlotId {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        s[*i] = self.0;
        *i += 1;
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let v = *s.get(*i).ok_or(Decode::Overflow)?;
        *i += 1;
        if v > isize::MAX as usize {
            return Err(Decode::Invalid);
        }
        Ok(Self::new(v))
    }
}

impl Wire for MappingId {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        s[*i] = self.0;
        *i += 1;
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let v = *s.get(*i).ok_or(Decode::Overflow)?;
        *i += 1;
        if v > isize::MAX as usize {
            return Err(Decode::Invalid);
        }
        Ok(Self::new(v))
    }
}

impl Wire for crate::model::cell::MappingTarget {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        let raw = match *self {
            Self::Cpu(team) => {
                assert!(team.get() <= isize::MAX as usize);
                team.get()
            }
            Self::Io(token) => {
                assert!(token.get() > 0 && token.get() <= isize::MAX as usize);
                token.get() | (1usize << (usize::BITS - 1))
            }
        };
        raw.pack(s, i);
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let raw = usize::unpack(s, i)?;
        let tag = 1usize << (usize::BITS - 1);
        if raw & tag == 0 {
            Ok(Self::Cpu(TeamId::new(raw)))
        } else if raw != tag {
            Ok(Self::Io(PendId::new(raw & !tag)))
        } else {
            Err(Decode::Invalid)
        }
    }
}
