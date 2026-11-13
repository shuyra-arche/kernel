//! Fixed-origin references for the next memory ABI. ABI v10 calls do not accept
//! these descriptors. Decoding checks representation, never live authority.

use super::FlatId;
use crate::abi::layout::{fetch_words, store_words};
use crate::model::{cell::CellLevel, identity::PendId};
use core::num::NonZeroU64;

/// A position in a Flat, independent of its Pend's effective window. Narrowing
/// cannot reinterpret `page`. The complete physical Page must fit the live grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotPosition {
    token: PendId,
    flat: FlatId,
    page: usize,
}

impl SlotPosition {
    pub const WIDTH: usize = 24;

    pub fn new(token: PendId, flat: FlatId, page: usize) -> Option<Self> {
        if token.get() == 0 || token.get() > isize::MAX as usize {
            return None;
        }
        crate::model::cell::pages(page, 1)?;
        Some(Self { token, flat, page })
    }

    pub const fn token(self) -> PendId {
        self.token
    }

    pub const fn flat(self) -> FlatId {
        self.flat
    }

    pub const fn page(self) -> usize {
        self.page
    }

    /// Geometry only, not an authorization check. `offset` is also measured
    /// from the Flat's fixed origin. The server must obtain all three arguments
    /// from current state and revalidate them at the operation's commit point.
    pub fn fits(self, offset: usize, count: usize, level: CellLevel) -> bool {
        level.fits_at(self.page, offset, count)
    }

    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        store_words([self.token.get(), self.flat.get(), self.page], &mut bytes);
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let [token, flat, page] = fetch_words(bytes)?;
        Self::new(PendId::mint(token), FlatId::new(flat)?, page)
    }
}

/// Residency/geometry revision, not a content version or permission. The next
/// memory ABI starts at one and refuses mutations when this sequence exhausts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotVersion(NonZeroU64);

impl SlotVersion {
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
