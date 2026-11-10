//! Slot and Memo identity, positions and handoff descriptors.

use super::SlotId;
use crate::abi::layout::{fetch_words, store_words};
use crate::address::BASE_PAGE_SIZE;
use crate::model::identity::PendId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellObject {
    Slot(SlotId),
    Memo(PendId),
}

impl CellObject {
    pub const WIDTH: usize = 32;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        store_words(self.words(), &mut bytes);
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Self::of(fetch_words(bytes)?)
    }

    pub(crate) fn words(self) -> [usize; 4] {
        match self {
            Self::Slot(id) => [0, id.get(), 0, 0],
            Self::Memo(token) => [1, token.get(), 0, 0],
        }
    }
    pub(crate) fn of(w: [usize; 4]) -> Option<Self> {
        match w {
            [0, id, 0, 0] if id != 0 && id <= isize::MAX as usize => {
                Some(Self::Slot(SlotId::new(id)))
            }
            [1, token, 0, 0] if token != 0 && token <= isize::MAX as usize => {
                Some(Self::Memo(PendId::new(token)))
            }
            _ => None,
        }
    }
}

/// Generation qualifies a Memo transition. Slot endpoints use generation zero;
/// they move the current unique holding and do not claim a content snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellPosition {
    pub object: CellObject,
    /// Zero-based position in 4 KiB ABI units, independent of physical Cell size
    /// and the leaf selected by a translation backend.
    pub page: usize,
    pub generation: u64,
}

impl CellPosition {
    pub const WIDTH: usize = 48;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut out = [0; Self::WIDTH];
        let o = self.object.words();
        store_words(
            [o[0], o[1], o[2], o[3], self.page, self.generation as usize],
            &mut out,
        );
        out
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let [a, b, c, d, page, generation] = fetch_words(bytes)?;
        let object = CellObject::of([a, b, c, d])?;
        page.checked_mul(BASE_PAGE_SIZE)?;
        if matches!(object, CellObject::Slot(_)) && generation != 0 {
            return None;
        }
        Some(Self {
            object,
            page,
            generation: generation as u64,
        })
    }
}

/// Transfer the unique holding. Push moves local → remote; Pull moves remote → local.
/// Both endpoints must be Slots and each requires independent transfer authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellHandoff {
    pub local: CellPosition,
    pub remote: CellPosition,
}

impl CellHandoff {
    pub const WIDTH: usize = CellPosition::WIDTH * 2;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        bytes[..48].copy_from_slice(&self.local.to_bytes());
        bytes[48..].copy_from_slice(&self.remote.to_bytes());
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::WIDTH {
            return None;
        }
        Some(Self {
            local: CellPosition::from_bytes(&bytes[..48])?,
            remote: CellPosition::from_bytes(&bytes[48..])?,
        })
    }
}

/// Insert a Page association without transferring its owning Slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellAssociation {
    pub source: CellPosition,
    pub target: CellPosition,
}

impl CellAssociation {
    pub const WIDTH: usize = CellPosition::WIDTH * 2;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        bytes[..48].copy_from_slice(&self.source.to_bytes());
        bytes[48..].copy_from_slice(&self.target.to_bytes());
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::WIDTH {
            return None;
        }
        Some(Self {
            source: CellPosition::from_bytes(&bytes[..48])?,
            target: CellPosition::from_bytes(&bytes[48..])?,
        })
    }
}
