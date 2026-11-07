use super::Decode;
use super::Wire;
use crate::model::cell::AcquireSource;
use crate::model::cell::Allocation;
use crate::model::cell::FlatReference;
use crate::model::cell::MappingControl;
use crate::model::cell::MappingId;
use crate::model::cell::MappingSelection;
use crate::model::cell::ReferenceRange;
use crate::model::cell::pages;
use crate::model::identity::PendId;
use crate::model::identity::TeamId;

impl Wire for AcquireSource {
    const WORDS: usize = AcquireSource::WORDS;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        let words = match *self {
            Self::Allocation(allocation) => [0, allocation.size, allocation.align, 0, 0],
            Self::Reference(range) => [1, range.reference.get(), range.offset, range.count, 0],
            Self::Source {
                token,
                offset,
                count,
            } => [2, token.get(), offset, count, 0],
            Self::Mapping {
                team,
                mapping,
                offset,
                count,
            } => [3, team.get(), mapping.get(), offset, count],
        };
        for word in words {
            word.pack(s, i);
        }
        debug_assert!(*i <= Self::WORDS);
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let mut w = [0usize; Self::WORDS];
        for word in &mut w {
            *word = usize::unpack(s, i)?;
        }
        let [tag, a, b, c, d] = w;
        if tag >= Self::TAGS {
            return Err(Decode::Invalid);
        }
        let window = |offset: usize, count: usize| {
            pages(offset, count)
                .map(|()| (offset, count))
                .ok_or(Decode::Invalid)
        };
        match tag {
            0 if c == 0 && d == 0 => Ok(Self::Allocation(
                Allocation::new(a, b).ok_or(Decode::Invalid)?,
            )),
            1 if d == 0 => Ok(Self::Reference(
                ReferenceRange::new(FlatReference::new(a), b, c).ok_or(Decode::Invalid)?,
            )),
            2 if d == 0 => {
                let (offset, count) = window(b, c)?;
                Ok(Self::Source {
                    token: PendId::new(a),
                    offset,
                    count,
                })
            }
            3 => {
                let (offset, count) = window(c, d)?;
                Ok(Self::Mapping {
                    team: TeamId::new(a),
                    mapping: MappingId::new(b),
                    offset,
                    count,
                })
            }
            _ => Err(Decode::Invalid),
        }
    }
}

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
        let reference = FlatReference::unpack(s, i)?;
        let offset = usize::unpack(s, i)?;
        let count = usize::unpack(s, i)?;
        Self::new(reference, offset, count).ok_or(Decode::Invalid)
    }
}

impl Wire for MappingSelection {
    const WORDS: usize = MappingSelection::WORDS;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        match *self {
            Self::Whole => {
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
            0 if offset == 0 && count == 0 => Ok(Self::Whole),
            1 => Self::pages(offset, count).ok_or(Decode::Invalid),
            _ => Err(Decode::Invalid),
        }
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

impl Wire for FlatReference {
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
