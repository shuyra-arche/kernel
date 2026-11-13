//! Slot or live Pend endpoints, generation-qualified positions and handoffs.

use super::SlotId;
use crate::abi::layout::{fetch_words, store_words};
use crate::address::BASE_PAGE_SIZE;
use crate::model::identity::PendId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellObject {
    Slot(SlotId),
    Pend(PendId),
}

impl CellObject {
    pub const WIDTH: usize = 16;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        store_words(self.words(), &mut bytes);
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Self::of(fetch_words(bytes)?)
    }

    pub(crate) fn words(self) -> [usize; 2] {
        match self {
            Self::Slot(id) => [0, id.get()],
            Self::Pend(token) => [1, token.get()],
        }
    }
    pub(crate) fn of([tag, id]: [usize; 2]) -> Option<Self> {
        if id == 0 || id > isize::MAX as usize {
            return None;
        }
        match tag {
            0 => Some(Self::Slot(SlotId::new(id))),
            1 => Some(Self::Pend(PendId::new(id))),
            _ => None,
        }
    }
}

/// Generation qualifies a mapping transition. Slot endpoints use generation zero;
/// they move the current unique holding and do not claim a content snapshot.
/// The server checks the actual family and generation of a Pend endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellPosition {
    pub object: CellObject,
    /// Zero-based position in 4 KiB ABI units, independent of physical Cell size
    /// and the leaf selected by a translation backend.
    pub page: usize,
    pub generation: u64,
}

impl CellPosition {
    pub const WIDTH: usize = 32;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut out = [0; Self::WIDTH];
        let [tag, id] = self.object.words();
        store_words([tag, id, self.page, self.generation as usize], &mut out);
        out
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let [tag, id, page, generation] = fetch_words(bytes)?;
        let object = CellObject::of([tag, id])?;
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
        bytes[..CellPosition::WIDTH].copy_from_slice(&self.local.to_bytes());
        bytes[CellPosition::WIDTH..].copy_from_slice(&self.remote.to_bytes());
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::WIDTH {
            return None;
        }
        let local = CellPosition::from_bytes(&bytes[..CellPosition::WIDTH])?;
        let remote = CellPosition::from_bytes(&bytes[CellPosition::WIDTH..])?;
        match (local.object, remote.object) {
            (CellObject::Slot(_), CellObject::Slot(_)) => Some(Self { local, remote }),
            _ => None,
        }
    }
}

/// Insert a Page association without transferring its owning Slot. Source Pend
/// authority is Flat/Share ASSOCIATE or mapping INSERT; the target requires mapping INSERT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellAssociation {
    pub source: CellPosition,
    pub target: CellPosition,
}

impl CellAssociation {
    pub const WIDTH: usize = CellPosition::WIDTH * 2;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        bytes[..CellPosition::WIDTH].copy_from_slice(&self.source.to_bytes());
        bytes[CellPosition::WIDTH..].copy_from_slice(&self.target.to_bytes());
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::WIDTH {
            return None;
        }
        Some(Self {
            source: CellPosition::from_bytes(&bytes[..CellPosition::WIDTH])?,
            target: CellPosition::from_bytes(&bytes[CellPosition::WIDTH..])?,
        })
    }
}
