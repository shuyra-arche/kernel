use super::RangeVersion;
use crate::{
    address::{PageCount, PageOffset},
    model::identity::PendId,
};

/// A Pend-bound range of pages in the original Span coordinate system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeSlice {
    range: PendId,
    offset: PageOffset,
    count: PageCount,
}

impl RangeSlice {
    pub const WIDTH: usize = 24;

    pub fn new(range: PendId, offset: PageOffset, count: PageCount) -> Option<Self> {
        (range.get() != 0).then_some(Self {
            range,
            offset,
            count,
        })
    }

    pub const fn range(self) -> PendId {
        self.range
    }

    pub const fn offset(self) -> PageOffset {
        self.offset
    }

    pub const fn count(self) -> PageCount {
        self.count
    }

    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        bytes[..8].copy_from_slice(&(self.range.get() as u64).to_le_bytes());
        bytes[8..16].copy_from_slice(&(self.offset.get() as u64).to_le_bytes());
        bytes[16..24].copy_from_slice(&(self.count.get() as u64).to_le_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let word = |at: usize| -> Option<u64> {
            Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
        };
        Self::new(
            PendId::mint(word(0)? as usize),
            PageOffset::new(word(8)? as usize),
            PageCount::new(word(16)? as usize)?,
        )
    }
}

/// Expected range version carried by Release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeRelease {
    pub target: RangeSlice,
    pub expected: RangeVersion,
}
