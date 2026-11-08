use crate::abi::layout::{WORD_BYTES, fetch_words, store_words};
use crate::abi::register::Wire;

use super::PendRange;
use crate::model::{
    identity::{Mark, PendId, TaskId},
    modifiable::Modifiable,
    view::{ViewId, ViewKind},
};
/// Eleven RV64 words. A View identity is descriptive; the holder's Pend grants access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendInfo {
    pub token: PendId,
    pub view: ViewId,
    pub kind: ViewKind,
    pub modifiable: Modifiable,
    pub owner: TaskId,
    pub lender: TaskId,
    pub mark: Mark,
    pub status: PendStatus,
    /// Effective source window for Share; Inherit denotes a non-ranged View.
    pub range: PendRange,
}

impl PendInfo {
    pub const WORDS: usize = 11;
    pub const SIZE: usize = Self::WORDS * WORD_BYTES;
    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        store_words(self.words(), &mut bytes);
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Self::from_words(fetch_words(bytes)?)
    }
    pub fn words(self) -> [usize; Self::WORDS] {
        let mut range = [0; 6];
        self.range.pack(&mut range, &mut 0);
        [
            self.token.get(),
            self.view.get(),
            self.kind as usize,
            self.modifiable.bits() as usize,
            self.owner.get(),
            self.lender.get(),
            self.mark.get() as usize,
            self.status as usize,
            range[0],
            range[1],
            range[2],
        ]
    }
    pub fn from_words(w: [usize; Self::WORDS]) -> Option<Self> {
        if w[0] == 0 || w[1] == 0 {
            return None;
        }
        Some(Self {
            token: PendId::new(w[0]),
            view: ViewId::new(w[1]),
            kind: ViewKind::of(w[2])?,
            modifiable: Modifiable::from_bits(u32::try_from(w[3]).ok()?)?,
            owner: TaskId::new(w[4]),
            lender: TaskId::new(w[5]),
            mark: Mark::new(w[6] as u64),
            status: PendStatus::of(w[7])?,
            range: PendRange::unpack(&[w[8], w[9], w[10], 0, 0, 0], &mut 0).ok()?,
        })
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendStatus {
    Active,
    Suspended,
    Retiring,
    Dead,
}
impl PendStatus {
    pub const fn of(value: usize) -> Option<Self> {
        match value {
            0 => Some(Self::Active),
            1 => Some(Self::Suspended),
            2 => Some(Self::Retiring),
            3 => Some(Self::Dead),
            _ => None,
        }
    }
}
