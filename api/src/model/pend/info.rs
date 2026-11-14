use super::{Limits, Target};
use crate::abi::layout::{WORD_BYTES, fetch_words, store_words};
use crate::model::identity::{Mark, PendId, TaskId};

/// An observation, never an authorization handle. Ranged scopes use resolved
/// absolute Pages; live permission stays in the Kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendInfo {
    pub token: PendId,
    pub target: Target,
    pub limits: Limits,
    pub owner: TaskId,
    /// Actual immediate delegating Task; zero for roots. Retained after lender
    /// exit. This identity does not establish live authorization or ownership.
    pub lender: TaskId,
    pub mark: Mark,
    pub status: PendStatus,
}
impl PendInfo {
    pub const WORDS: usize = 16;
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
        let [kind, id] = self.target.words();
        let mut words = [0; Self::WORDS];
        words[..7].copy_from_slice(&[
            self.token.get(),
            kind,
            id,
            self.owner.get(),
            self.lender.get(),
            self.mark.get() as usize,
            self.status as usize,
        ]);
        words[7..].copy_from_slice(&self.limits.words());
        words
    }
    pub fn from_words(w: [usize; Self::WORDS]) -> Option<Self> {
        if w[0] == 0 {
            return None;
        }
        let target = Target::of([w[1], w[2]])?;
        let limits = Limits::of(w[7..].try_into().ok()?)?;
        if !target.accepts(limits.operations)
            || matches!(limits.scope.range(), Some(super::PendRange::Inherit))
        {
            return None;
        }
        Some(Self {
            token: PendId::new(w[0]),
            target,
            limits,
            owner: TaskId::new(w[3]),
            lender: TaskId::new(w[4]),
            mark: Mark::new(w[5] as u64),
            status: PendStatus::of(w[6])?,
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
