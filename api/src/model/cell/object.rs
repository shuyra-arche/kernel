//! Slot and Memo identity, positions and handoff descriptors.

use super::{MappingId, MappingTarget, PAGE_SIZE, SlotId};
use crate::abi::{
    layout::{fetch_words, store_words},
    register::Wire,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellObject {
    Slot(SlotId),
    Memo {
        target: MappingTarget,
        mapping: MappingId,
    },
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
            Self::Memo { target, mapping } => {
                let mut words = [0; 6];
                target.pack(&mut words, &mut 0);
                [1, words[0], words[1], mapping.get()]
            }
        }
    }
    pub(crate) fn of(w: [usize; 4]) -> Option<Self> {
        match w {
            [0, id, 0, 0] if id != 0 && id <= isize::MAX as usize => {
                Some(Self::Slot(SlotId::new(id)))
            }
            [1, tag, 0, mapping] if mapping != 0 && mapping <= isize::MAX as usize => {
                Some(Self::Memo {
                    target: MappingTarget::unpack(&[tag, 0, 0, 0, 0, 0], &mut 0).ok()?,
                    mapping: MappingId::new(mapping),
                })
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
        page.checked_mul(PAGE_SIZE)?;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellHandoff {
    pub source: CellPosition,
    pub target: CellPosition,
}

impl CellHandoff {
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
