//! Next-memory-ABI contracts and domain calls. These do not implement RegisterCall
//! or enable ABI 11 in the running v10 dispatcher. Every decoded token
//! still requires caller-local live authorization at the server commit point.

mod call;
pub mod journal;
pub use call::{Call, Frame, flat, nest};
mod observation;
mod reply;
mod request;

pub use observation::{Observation, Query, Snapshot};
pub use reply::{Delivery, Outcome, Reply, Value};
pub use request::{Action, Maintenance, Request, Source};

use core::num::NonZeroU64;

/// Proposed activation version; distinct from the currently supported ABI.
pub const VERSION: u64 = 11;

/// Proposed per-Task result window for these requests. Server integration must
/// reserve capacity before admission and must never evict a Pending operation.
pub const RESULT_CAPACITY: usize = 16;

/// Caller-chosen monotonic sequence, scoped by the actual calling Task. Never
/// infer the actor from this value or from a request's supplied object IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestId(NonZeroU64);

impl RequestId {
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum Domain {
    Flat = 12,
    Nest = 13,
}

impl Domain {
    fn of(value: u64) -> Option<Self> {
        match value {
            12 => Some(Self::Flat),
            13 => Some(Self::Nest),
            _ => None,
        }
    }
}

/// Only operations whose descriptor is implemented here. No wildcard operation,
/// retired Cell domain or generic transaction command is accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opcode {
    Create(Domain),
    Delete(Domain),
    Inspect(Domain),
    Reserve,
    Release,
    Occupy,
    Vacate,
    Block,
    Allow,
    Push,
    Pull,
    Split,
    Merge,
    Insert,
    Remove,
    Publish,
    Retract,
    Protect,
    FaultRoute,
}

impl Opcode {
    /// Proposed trap number. Merely computing it does not enable dispatch.
    pub const fn call_number(self) -> u64 {
        (VERSION << 48) | ((self.domain() as u64) << 32) | self.slot()
    }

    pub fn from_call_number(number: u64) -> Option<Self> {
        if number >> 48 != VERSION {
            return None;
        }
        Self::of((number >> 32) & 0xffff, number & 0xffff_ffff)
    }

    pub const fn domain(self) -> Domain {
        match self {
            Self::Create(domain) | Self::Delete(domain) | Self::Inspect(domain) => domain,
            Self::Occupy
            | Self::Vacate
            | Self::Block
            | Self::Allow
            | Self::Push
            | Self::Pull
            | Self::Split
            | Self::Merge => Domain::Flat,
            Self::Reserve
            | Self::Release
            | Self::Insert
            | Self::Remove
            | Self::Publish
            | Self::Retract
            | Self::Protect
            | Self::FaultRoute => Domain::Nest,
        }
    }

    pub const fn slot(self) -> u64 {
        match self {
            Self::Create(_) => 16,
            Self::Delete(_) => 17,
            Self::Inspect(_) => 18,
            Self::Occupy | Self::Reserve => 19,
            Self::Vacate | Self::Release => 20,
            Self::Block | Self::Insert => 21,
            Self::Allow | Self::Remove => 22,
            Self::Push | Self::Publish => 23,
            Self::Pull | Self::Retract => 24,
            Self::Split | Self::Protect => 25,
            Self::Merge | Self::FaultRoute => 26,
        }
    }

    pub fn of(domain: u64, slot: u64) -> Option<Self> {
        Some(match (Domain::of(domain)?, slot) {
            (domain, 16) => Self::Create(domain),
            (domain, 17) => Self::Delete(domain),
            (domain, 18) => Self::Inspect(domain),
            (Domain::Flat, 19) => Self::Occupy,
            (Domain::Flat, 20) => Self::Vacate,
            (Domain::Flat, 21) => Self::Block,
            (Domain::Flat, 22) => Self::Allow,
            (Domain::Flat, 23) => Self::Push,
            (Domain::Flat, 24) => Self::Pull,
            (Domain::Flat, 25) => Self::Split,
            (Domain::Flat, 26) => Self::Merge,
            (Domain::Nest, 19) => Self::Reserve,
            (Domain::Nest, 20) => Self::Release,
            (Domain::Nest, 21) => Self::Insert,
            (Domain::Nest, 22) => Self::Remove,
            (Domain::Nest, 23) => Self::Publish,
            (Domain::Nest, 24) => Self::Retract,
            (Domain::Nest, 25) => Self::Protect,
            (Domain::Nest, 26) => Self::FaultRoute,
            _ => return None,
        })
    }
}

/// Failure causes for the new descriptors, not v10 FlatFail/NestFail codes.
/// Whether effects were accepted is represented separately by Outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i64)]
pub enum Failure {
    Denied = -1,
    Dead = -2,
    Busy = -3,
    OutOfMemory = -4,
    NotAligned = -5,
    NoRegion = -6,
    Stale = -7,
    SourceEmpty = -8,
    DestinationOccupied = -9,
    NotTransferable = -10,
    Unsupported = -11,
    VersionExhausted = -12,
    RequestMismatch = -13,
    ResultExpired = -14,
    ReplyFault = -15,
    Invalid = -16,
    Hardware = -17,
    TimedOut = -18,
    NoHandler = -19,
    HandlerGone = -20,
    QueueFull = -21,
    FaultExpired = -22,
}

impl Failure {
    pub const fn code(self) -> i64 {
        self as i64
    }

    pub fn from_code(code: i64) -> Option<Self> {
        Some(match code {
            -1 => Self::Denied,
            -2 => Self::Dead,
            -3 => Self::Busy,
            -4 => Self::OutOfMemory,
            -5 => Self::NotAligned,
            -6 => Self::NoRegion,
            -7 => Self::Stale,
            -8 => Self::SourceEmpty,
            -9 => Self::DestinationOccupied,
            -10 => Self::NotTransferable,
            -11 => Self::Unsupported,
            -12 => Self::VersionExhausted,
            -13 => Self::RequestMismatch,
            -14 => Self::ResultExpired,
            -15 => Self::ReplyFault,
            -16 => Self::Invalid,
            -17 => Self::Hardware,
            -18 => Self::TimedOut,
            -19 => Self::NoHandler,
            -20 => Self::HandlerGone,
            -21 => Self::QueueFull,
            -22 => Self::FaultExpired,
            _ => return None,
        })
    }
}

/// A small checked cursor shared only by these bounded descriptor codecs.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn bytes<const N: usize>(&mut self) -> Option<&'a [u8; N]> {
        let (head, tail) = self.0.split_at_checked(N)?;
        self.0 = tail;
        head.try_into().ok()
    }

    fn word(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(*self.bytes()?))
    }

    fn size(&mut self) -> Option<usize> {
        usize::try_from(self.word()?).ok()
    }

    fn token(&mut self) -> Option<crate::model::identity::PendId> {
        let value = self.size()?;
        valid_token(value).then(|| crate::model::identity::PendId::mint(value))
    }

    fn mapping(&mut self) -> Option<crate::model::cell::MappingId> {
        let value = self.size()?;
        valid_token(value).then(|| crate::model::cell::MappingId::new(value))
    }

    fn finish(self) -> Option<()> {
        self.0.iter().all(|byte| *byte == 0).then_some(())
    }
}

struct Writer<'a>(&'a mut [u8]);

impl Writer<'_> {
    fn bytes(&mut self, bytes: &[u8]) -> Option<()> {
        if bytes.len() > self.0.len() {
            return None;
        }
        let (head, tail) = core::mem::take(&mut self.0).split_at_mut(bytes.len());
        head.copy_from_slice(bytes);
        self.0 = tail;
        Some(())
    }

    fn word(&mut self, word: u64) -> Option<()> {
        self.bytes(&word.to_le_bytes())
    }
}

fn valid_token(value: usize) -> bool {
    value > 0 && value <= isize::MAX as usize
}
