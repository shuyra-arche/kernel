//! Fixed Mapping-origin positions for the next memory ABI. Position identity is
//! separate from the expected generation; Inspect need not know a generation.

use super::NestId;
use crate::abi::layout::{fetch_words, store_words};
use crate::model::{
    cell::{CellLevel, MappingId},
    identity::PendId,
};
use core::num::NonZeroU64;

/// A Memo coordinate relative to the original Mapping, never the current Pend
/// window or virtual address. ABI v10 calls do not accept this descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoPosition {
    token: PendId,
    nest: NestId,
    mapping: MappingId,
    page: usize,
}

impl MemoPosition {
    pub const WIDTH: usize = 32;

    pub fn new(token: PendId, nest: NestId, mapping: MappingId, page: usize) -> Option<Self> {
        if token.get() == 0
            || token.get() > isize::MAX as usize
            || mapping.get() == 0
            || mapping.get() > isize::MAX as usize
        {
            return None;
        }
        crate::model::cell::pages(page, 1)?;
        Some(Self {
            token,
            nest,
            mapping,
            page,
        })
    }

    pub const fn token(self) -> PendId {
        self.token
    }

    pub const fn nest(self) -> NestId {
        self.nest
    }

    pub const fn mapping(self) -> MappingId {
        self.mapping
    }

    pub const fn page(self) -> usize {
        self.page
    }

    /// Geometry only. `offset` is relative to the original Mapping, even after
    /// Narrow. Current live scope, source geometry and destination alignment
    /// still have to be checked together by the server at commit.
    pub fn fits(self, offset: usize, count: usize, level: CellLevel) -> bool {
        level.fits_at(self.page, offset, count)
    }

    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        store_words(
            [
                self.token.get(),
                self.nest.get(),
                self.mapping.get(),
                self.page,
            ],
            &mut bytes,
        );
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let [token, nest, mapping, page] = fetch_words(bytes)?;
        if mapping == 0 || mapping > isize::MAX as usize {
            return None;
        }
        Self::new(
            PendId::mint(token),
            NestId::new(nest)?,
            MappingId::new(mapping),
            page,
        )
    }
}

/// Logical association/publication generation, never a content version. Values
/// start at one; exhaustion rejects new mutations rather than wrapping.
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
