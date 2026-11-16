//! Native ABI 11 Flat/Nest contracts.
//! No per-call ABI 10 fallback exists. Each token still requires live caller-local
//! authorization at the Server's commit boundary.

mod call;
pub mod journal;
pub use call::{Call, Frame};
mod observation;
mod reply;
mod request;

pub use observation::{Observation, Query, Snapshot};
pub use reply::{Delivery, Outcome, Reply, Value};
pub use request::{Action, Maintenance, Request, Source};

use core::num::NonZeroU64;

/// The same version governs calls, memory descriptors, Ledger and Capsule ABI.
pub const VERSION: u64 = crate::abi::VERSION as u64;

/// Rights for native holding and span operations.
pub mod rights {
    use crate::model::pend::{FlatRights, Limits, Operations};
    pub const OCCUPY: FlatRights = FlatRights::OCCUPY;
    pub const VACATE: FlatRights = FlatRights::VACATE;
    pub const RESHAPE: FlatRights = FlatRights::RESHAPE;
    /// Span-only native fault control.
    pub const FAULT: crate::model::pend::NestRights = crate::model::pend::NestRights::FAULT;

    /// Select already-issued Flat rights using the common attenuation rule.
    /// Unknown bits and widening of access, scope or distribution reject.
    pub fn selects(parent: Limits, selected: Limits) -> bool {
        let (Operations::Flat(_), Operations::Flat(_)) = (parent.operations, selected.operations)
        else {
            return false;
        };
        parent.permits_narrowing(selected)
    }
}

/// Fixed per-Task result capacity. Server reserves before admission and never
/// evicts a Pending operation to admit a new request.
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

pub mod flat {
    pub use super::call::flat::Call;
    /// Flat-domain operation slots. The wire domain is still carried by the
    /// shared envelope; these are not mixed with Nest operations.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Opcode {
        Create,
        Delete,
        Inspect,
        Occupy,
        Vacate,
        Block,
        Allow,
        Push,
        Pull,
        Split,
        Merge,
    }
}

pub mod nest {
    pub use super::call::nest::Call;
    /// Nest-domain operation slots. Reserve/Release name address occupancy and
    /// range release; the internal Span is not a public operation handle.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Opcode {
        Create,
        Delete,
        Inspect,
        Reserve,
        Release,
        Insert,
        Remove,
        Publish,
        Retract,
        Protect,
        FaultRoute,
        ReleaseRange,
    }
}

/// Shared wire opcode envelope. Domain actions keep their own enums; this type
/// only maps a domain opcode to the fixed trap slot table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opcode {
    Flat(flat::Opcode),
    Nest(nest::Opcode),
}

#[allow(non_snake_case)]
impl Opcode {
    pub const fn Create(domain: Domain) -> Self {
        match domain {
            Domain::Flat => Self::Flat(flat::Opcode::Create),
            Domain::Nest => Self::Nest(nest::Opcode::Create),
        }
    }

    pub const fn Delete(domain: Domain) -> Self {
        match domain {
            Domain::Flat => Self::Flat(flat::Opcode::Delete),
            Domain::Nest => Self::Nest(nest::Opcode::Delete),
        }
    }

    pub const fn Inspect(domain: Domain) -> Self {
        Self::inspect(domain)
    }

    pub const CreateSpan: Self = Self::Nest(nest::Opcode::Reserve);
    pub const DeleteSpan: Self = Self::Nest(nest::Opcode::Release);
    pub const ReleaseRange: Self = Self::Nest(nest::Opcode::ReleaseRange);
    pub const Occupy: Self = Self::Flat(flat::Opcode::Occupy);
    pub const Vacate: Self = Self::Flat(flat::Opcode::Vacate);
    pub const Block: Self = Self::Flat(flat::Opcode::Block);
    pub const Allow: Self = Self::Flat(flat::Opcode::Allow);
    pub const Push: Self = Self::Flat(flat::Opcode::Push);
    pub const Pull: Self = Self::Flat(flat::Opcode::Pull);
    pub const Split: Self = Self::Flat(flat::Opcode::Split);
    pub const Merge: Self = Self::Flat(flat::Opcode::Merge);
    pub const Insert: Self = Self::Nest(nest::Opcode::Insert);
    pub const Remove: Self = Self::Nest(nest::Opcode::Remove);
    pub const Publish: Self = Self::Nest(nest::Opcode::Publish);
    pub const Retract: Self = Self::Nest(nest::Opcode::Retract);
    pub const Protect: Self = Self::Nest(nest::Opcode::Protect);
    pub const FaultRoute: Self = Self::Nest(nest::Opcode::FaultRoute);
}

impl Opcode {
    pub const fn inspect(domain: Domain) -> Self {
        match domain {
            Domain::Flat => Self::Flat(flat::Opcode::Inspect),
            Domain::Nest => Self::Nest(nest::Opcode::Inspect),
        }
    }

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
            Self::Flat(_) => Domain::Flat,
            Self::Nest(_) => Domain::Nest,
        }
    }

    pub const fn slot(self) -> u64 {
        match self {
            Self::Flat(op) => match op {
                flat::Opcode::Create => 16,
                flat::Opcode::Delete => 17,
                flat::Opcode::Inspect => 18,
                flat::Opcode::Occupy => 19,
                flat::Opcode::Vacate => 20,
                flat::Opcode::Block => 21,
                flat::Opcode::Allow => 22,
                flat::Opcode::Push => 23,
                flat::Opcode::Pull => 24,
                flat::Opcode::Split => 25,
                flat::Opcode::Merge => 26,
            },
            Self::Nest(op) => match op {
                nest::Opcode::Create => 16,
                nest::Opcode::Delete => 17,
                nest::Opcode::Inspect => 18,
                nest::Opcode::Reserve => 19,
                nest::Opcode::Release => 20,
                nest::Opcode::Insert => 21,
                nest::Opcode::Remove => 22,
                nest::Opcode::Publish => 23,
                nest::Opcode::Retract => 24,
                nest::Opcode::Protect => 25,
                nest::Opcode::FaultRoute => 26,
                nest::Opcode::ReleaseRange => 27,
            },
        }
    }

    pub fn of(domain: u64, slot: u64) -> Option<Self> {
        Some(match (Domain::of(domain)?, slot) {
            (Domain::Flat, 16) => Self::Flat(flat::Opcode::Create),
            (Domain::Flat, 17) => Self::Flat(flat::Opcode::Delete),
            (Domain::Flat, 18) => Self::Flat(flat::Opcode::Inspect),
            (Domain::Flat, 19) => Self::Flat(flat::Opcode::Occupy),
            (Domain::Flat, 20) => Self::Flat(flat::Opcode::Vacate),
            (Domain::Flat, 21) => Self::Flat(flat::Opcode::Block),
            (Domain::Flat, 22) => Self::Flat(flat::Opcode::Allow),
            (Domain::Flat, 23) => Self::Flat(flat::Opcode::Push),
            (Domain::Flat, 24) => Self::Flat(flat::Opcode::Pull),
            (Domain::Flat, 25) => Self::Flat(flat::Opcode::Split),
            (Domain::Flat, 26) => Self::Flat(flat::Opcode::Merge),
            (Domain::Nest, 16) => Self::Nest(nest::Opcode::Create),
            (Domain::Nest, 17) => Self::Nest(nest::Opcode::Delete),
            (Domain::Nest, 18) => Self::Nest(nest::Opcode::Inspect),
            (Domain::Nest, 19) => Self::Nest(nest::Opcode::Reserve),
            (Domain::Nest, 20) => Self::Nest(nest::Opcode::Release),
            (Domain::Nest, 21) => Self::Nest(nest::Opcode::Insert),
            (Domain::Nest, 22) => Self::Nest(nest::Opcode::Remove),
            (Domain::Nest, 23) => Self::Nest(nest::Opcode::Publish),
            (Domain::Nest, 24) => Self::Nest(nest::Opcode::Retract),
            (Domain::Nest, 25) => Self::Nest(nest::Opcode::Protect),
            (Domain::Nest, 26) => Self::Nest(nest::Opcode::FaultRoute),
            (Domain::Nest, 27) => Self::Nest(nest::Opcode::ReleaseRange),
            _ => return None,
        })
    }
}

/// Native descriptor failures, distinct from ABI 10 FlatFail/NestFail codes.
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

    fn span(&mut self) -> Option<crate::model::nest::SpanId> {
        let value = self.size()?;
        valid_token(value).then(|| crate::model::nest::SpanId::new(value))
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
