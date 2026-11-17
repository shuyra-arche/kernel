//! Pend-bound Memo positions for the next memory ABI.

use crate::abi::layout::{fetch_words, store_words};
use crate::model::{cell::CellLevel, identity::PendId};
use core::num::NonZeroU64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoPosition {
    range: PendId,
    page: usize,
}

impl MemoPosition {
    pub const WIDTH: usize = 16;

    pub fn new(range: PendId, page: usize) -> Option<Self> {
        if range.get() == 0 || range.get() > isize::MAX as usize {
            return None;
        }
        crate::model::cell::pages(page, 1)?;
        Some(Self { range, page })
    }

    pub const fn range(self) -> PendId {
        self.range
    }

    pub const fn page(self) -> usize {
        self.page
    }

    pub fn fits(self, offset: usize, count: usize, level: CellLevel) -> bool {
        level.fits_at(self.page, offset, count)
    }

    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        store_words([self.range.get(), self.page], &mut bytes);
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let [range, page] = fetch_words(bytes)?;
        Self::new(PendId::mint(range), page)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MemoGeneration(NonZeroU64);

impl MemoGeneration {
    pub const WIDTH: usize = 8;
    pub const INITIAL: Self = Self(NonZeroU64::MIN);

    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }

    pub fn next(self) -> Option<Self> {
        Self::new(self.get().checked_add(1)?)
    }

    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        self.get().to_le_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Self::new(u64::from_le_bytes(bytes.try_into().ok()?))
    }
}
