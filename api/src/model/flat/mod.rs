//! Stable flat identity. A numeric identity is descriptive and never grants access.
mod maintenance;
mod position;
mod reshape;
mod status;

pub use maintenance::{BlockGeneration, BlockPhase, BlockRef};
pub use position::{SlotPosition, SlotVersion};
pub use reshape::Reshape;
pub use status::{BlockStatus, FlatPhase, FlatStatus, FlatVersion, SlotPhase, SlotStatus};

use crate::abi::register::{Decode, Wire};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct FlatId(usize);
impl FlatId {
    pub const fn new(value: usize) -> Option<Self> {
        if value > 0 && value <= isize::MAX as usize {
            Some(Self(value))
        } else {
            None
        }
    }
    pub const fn get(self) -> usize {
        self.0
    }
}
impl Wire for FlatId {
    const WORDS: usize = 1;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        self.0.pack(words, at);
    }
    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, Decode> {
        Self::new(usize::unpack(words, at)?).ok_or(Decode::Invalid)
    }
}

/// Effective Flat window; neither the identity nor these coordinates authorize access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlatInfo {
    pub level: crate::model::cell::CellLevel,
    pub id: FlatId,
    pub offset: usize,
    pub count: usize,
}
impl FlatInfo {
    pub const WIDTH: usize = 32;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut b = [0; Self::WIDTH];
        crate::abi::layout::store_words(
            [self.id.get(), self.offset, self.count, self.level.get()],
            &mut b,
        );
        b
    }
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        let [id, offset, count, level] = crate::abi::layout::fetch_words(b)?;
        if count == 0 || offset.checked_add(count).is_none() {
            return None;
        }
        Some(Self {
            id: FlatId::new(id)?,
            level: crate::model::cell::CellLevel::new(level)?,
            offset,
            count,
        })
    }
}
