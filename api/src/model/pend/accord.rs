use crate::abi::layout::{fetch_words, store_words};

use super::Limits;
use crate::model::identity::{Mark, PendId, TaskId};

/// A local derivation and a delegation use the same operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccordTarget {
    Caller,
    Task(TaskId),
}

/// Immutable request snapshot consumed by Pend.Accord.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccordRequest {
    pub src: PendId,
    pub target: AccordTarget,
    pub limits: Limits,
    /// NONE inherits the parent's mark.
    pub mark: Mark,
}

impl AccordRequest {
    /// Source, recipient tag/ID, mark, then nine checked permission words.
    pub const WIDTH: usize = 104;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let (tag, id) = match self.target {
            AccordTarget::Caller => (0, 0),
            AccordTarget::Task(id) => (1, id.get()),
        };
        let mut bytes = [0; Self::WIDTH];
        store_words(
            [self.src.get(), tag, id, self.mark.get() as usize],
            &mut bytes[..32],
        );
        store_words(self.limits.words(), &mut bytes[32..]);
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::WIDTH {
            return None;
        }
        let [src, tag, id, mark] = fetch_words(&bytes[..32])?;
        if src == 0 {
            return None;
        }
        let target = match (tag, id) {
            (0, 0) => AccordTarget::Caller,
            (1, id) if id != 0 => AccordTarget::Task(TaskId::new(id)),
            _ => return None,
        };
        Some(Self {
            src: PendId::new(src),
            target,
            mark: Mark::new(mark as u64),
            limits: Limits::of(fetch_words(&bytes[32..])?)?,
        })
    }
}

/// A narrowing request is read once before entering the authorization commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NarrowRequest {
    pub token: PendId,
    pub limits: Limits,
}
impl NarrowRequest {
    pub const WIDTH: usize = 80;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        store_words([self.token.get()], &mut bytes[..8]);
        store_words(self.limits.words(), &mut bytes[8..]);
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::WIDTH {
            return None;
        }
        let [token] = fetch_words(&bytes[..8])?;
        if token == 0 {
            return None;
        }
        Some(Self {
            token: PendId::new(token),
            limits: Limits::of(fetch_words(&bytes[8..])?)?,
        })
    }
}
