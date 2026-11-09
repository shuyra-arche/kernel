//! Physical allocation geometry; coordinates elsewhere remain in base pages.
use super::CellLevel;
use crate::abi::register::{Decode, Wire};
use crate::address::BASE_PAGE_SIZE;
/// Each Cell is physically contiguous and aligned to its level. Separate Cells
/// need not be adjacent. Base-level requests retain their rounding behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allocation {
    pub size: usize,
    pub align: usize,
    pub level: CellLevel,
}
impl Allocation {
    pub const WORDS: usize = 3;
    pub fn new(size: usize, align: usize) -> Option<Self> {
        let value = Self {
            size,
            align,
            level: CellLevel::BASE,
        };
        value.bytes()?;
        Some(value)
    }
    pub fn at_level(level: CellLevel, count: usize) -> Option<Self> {
        let align = level.bytes()?;
        let value = Self {
            size: align.checked_mul(count)?,
            align,
            level,
        };
        value.bytes()?;
        Some(value)
    }
    pub fn bytes(self) -> Option<usize> {
        if self.size < BASE_PAGE_SIZE || !self.align.is_power_of_two() {
            return None;
        }
        let unit = self.level.bytes()?;
        if self.level != CellLevel::BASE && (self.align != unit || !self.size.is_multiple_of(unit))
        {
            return None;
        }
        self.size.checked_next_multiple_of(unit)
    }
}
impl Wire for Allocation {
    const WORDS: usize = Allocation::WORDS;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        self.size.pack(s, i);
        self.align.pack(s, i);
        self.level.get().pack(s, i);
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let value = Self {
            size: usize::unpack(s, i)?,
            align: usize::unpack(s, i)?,
            level: CellLevel::new(usize::unpack(s, i)?).ok_or(Decode::Invalid)?,
        };
        value.bytes().ok_or(Decode::Invalid)?;
        Ok(value)
    }
}
