//! Clone queue limits, conditions and delivered payloads.

use crate::abi::register::{Decode, Wire};
use crate::model::identity::{PendId, TaskId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloneLimits {
    pub max_len: usize,
    pub max_messages: usize,
    pub max_bytes: usize,
}

impl Default for CloneLimits {
    fn default() -> Self {
        Self {
            max_len: isize::MAX as usize,
            max_messages: 4,
            max_bytes: usize::MAX,
        }
    }
}

impl CloneLimits {
    pub fn valid(self) -> bool {
        self.max_len > 0
            && self.max_len <= isize::MAX as usize
            && self.max_messages > 0
            && self.max_bytes >= self.max_len
    }
}

impl Wire for CloneLimits {
    const WORDS: usize = 3;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        self.max_len.pack(words, at);
        self.max_messages.pack(words, at);
        self.max_bytes.pack(words, at);
    }

    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, Decode> {
        let limits = Self {
            max_len: usize::unpack(words, at)?,
            max_messages: usize::unpack(words, at)?,
            max_bytes: usize::unpack(words, at)?,
        };
        if limits.valid() {
            Ok(limits)
        } else {
            Err(Decode::Invalid)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CloneCondition {
    Pull,
    Push,
    Empty,
}

impl Wire for CloneCondition {
    const WORDS: usize = 1;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        (*self as usize).pack(words, at);
    }

    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, Decode> {
        match usize::unpack(words, at)? {
            0 => Ok(Self::Pull),
            1 => Ok(Self::Push),
            2 => Ok(Self::Empty),
            _ => Err(Decode::Invalid),
        }
    }
}

/// One independently owned delivery. Open its read-only Pend through Share;
/// closing the originating queue does not revoke this copied payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PullOutcome {
    pub token: PendId,
    pub len: usize,
    pub sender: TaskId,
}

impl crate::abi::reply::FromTriple for PullOutcome {
    fn from_triple(token: usize, len: usize, sender: usize) -> Self {
        assert!(
            token > 0 && token <= isize::MAX as usize && len > 0,
            "invalid payload delivery"
        );
        Self {
            token: PendId::new(token),
            len,
            sender: TaskId::new(sender),
        }
    }
}
