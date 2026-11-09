//! One control endpoint supports explicit triggering and authorized source routing.
use crate::model::identity::PendId;
use crate::model::identity::TaskId;
use crate::model::sign::SignSource;
use crate::model::wait::Wait;

pub type SignResult<T> = Result<T, erra::Error<SignFail>>;

#[kernel_api_macros::envcall(class = 11, call = SignCall)]
pub mod operations {
    #[error_domain]
    pub enum SignFail {
        Denied = -1,
        Dead = -2,
        #[busy]
        Busy = -3,
        OutOfMemory = -4,
        HandedOver = -5,
    }
    #[error_domain]
    pub enum SignEventFail {
        Denied = -1,
        Dead = -2,
        #[busy]
        Busy = -3,
        OutOfMemory = -4,
        HandedOver = -5,
        Gone = -6,
    }
    #[operation(slot = 0, output = super::PendId, error = SignFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Create {
        pub license: super::PendId,
    }
    #[operation(slot = 1, output = (), error = SignFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Delete {
        pub token: super::PendId,
    }
    /// Observe a retained direct trigger or a ready source; does not consume it.
    /// A timeout only ends this wait, without cancelling the source operation.
    #[operation(slot = 2, output = super::SignReply, error = SignFail, reply = 3)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Listen {
        pub token: super::PendId,
        pub wait: super::Wait,
    }
    /// Capacity is one pending direct trigger; a second trigger returns Busy.
    #[operation(slot = 3, output = (), error = SignFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Trigger {
        pub token: super::PendId,
    }
    /// Clear a direct trigger. Routed events retain their source's own semantics.
    #[operation(slot = 4, output = (), error = SignFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Consume {
        pub token: super::PendId,
        pub sequence: u64,
    }
    /// Register observation of a source already authorized to this Task.
    #[operation(slot = 5, output = (), error = SignFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Attach {
        pub token: super::PendId,
        pub source: super::SignSource,
        /// Exact Memo generation, or zero for every non-Memo source.
        pub generation: u64,
    }
    #[operation(slot = 6, output = (), error = SignFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Detach {
        pub token: super::PendId,
        pub source: super::SignSource,
    }
    /// Bind a controlled admitted server using its live Trigger grant for this
    /// endpoint. Ordinary Listen remains independent of service dependencies.
    #[operation(slot = 10, output = (), error = SignFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Assign {
        pub token: super::PendId,
        pub server: super::TaskId,
        pub grant: super::PendId,
    }
    #[operation(slot = 11, output = (), error = SignFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Resign {
        pub token: super::PendId,
    }
    /// Wait on the assigned server, with bounded transitive priority inheritance.
    /// Caller and server must be admitted on the same Hart. Budgets stay separate.
    #[operation(slot = 12, output = super::SignReply, error = SignFail, reply = 3)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Await {
        pub token: super::PendId,
        pub wait: super::Wait,
    }
}
pub use operations::{SignCall, SignEventFail, SignFail};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignReply {
    Pending,
    Triggered {
        sequence: u64,
    },
    Source {
        source: SignSource,
        fail: Option<SignEventFail>,
    },
}

impl SignReply {
    pub fn is_none(self) -> bool {
        self == Self::Pending
    }

    pub fn words(self) -> [usize; 3] {
        match self {
            Self::Pending => [0; 3],
            Self::Triggered { sequence } => [9, sequence as usize, 0],
            Self::Source { source, fail } => {
                let mut words = source.words();
                words[0] |=
                    fail.map_or(0, |e| (-crate::abi::error::FailCode::code(e)) as usize) << 8;
                words
            }
        }
    }

    pub fn of(mut words: [usize; 3]) -> Option<Self> {
        if words == [0; 3] {
            return Some(Self::Pending);
        }
        if words[0] == 9 && words[1] != 0 && words[2] == 0 {
            return Some(Self::Triggered {
                sequence: words[1] as u64,
            });
        }
        let fail = match words[0] >> 8 {
            0 => None,
            1 => Some(SignEventFail::Denied),
            2 => Some(SignEventFail::Dead),
            3 => Some(SignEventFail::Busy),
            4 => Some(SignEventFail::OutOfMemory),
            5 => Some(SignEventFail::HandedOver),
            6 => Some(SignEventFail::Gone),
            _ => return None,
        };
        words[0] &= 255;
        Some(Self::Source {
            source: SignSource::of(words)?,
            fail,
        })
    }
}

impl crate::abi::reply::FromTriple for SignReply {
    fn from_triple(a: usize, b: usize, c: usize) -> Self {
        Self::of([a, b, c]).expect("invalid Sign Listen reply")
    }
}
