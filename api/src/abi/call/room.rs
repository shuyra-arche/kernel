//! Scheduling and monotonic time. All deadlines are absolute nanoseconds since boot.
use crate::address::VirtAddr;
use crate::model::{clock::ClockSource, wait::Wait};
pub type RoomResult<T> = Result<T, erra::Error<RoomFail>>;
#[kernel_api_macros::envcall(class = 0, call = RoomCall)]
pub mod operations {
    #[error_domain]
    pub enum RoomFail {
        Dead = -1,
        OutOfMemory = -2,
        #[busy]
        Busy = -3,
        Denied = -4,
        Invalid = -5,
    }
    #[operation(slot = 0, output = (), infallible)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Yield;
    /// Returns no earlier than the deadline; an elapsed deadline returns immediately.
    #[operation(slot = 1, output = (), error = RoomFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct SleepUntil {
        pub at: u64,
    }
    /// Compare and enqueue atomically with WakeValue in the caller's address space.
    /// False means timeout. True requires the caller to recheck its predicate.
    /// Keep the aligned AtomicU32 and its mapping alive until all waiters return.
    #[operation(slot = 2, output = bool, error = RoomFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct WaitValue {
        pub address: super::VirtAddr,
        pub expected: u32,
        pub wait: super::Wait,
    }
    /// Wake one or all waiters after publishing the predicate; notifications are not retained.
    #[operation(slot = 3, output = usize, error = RoomFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct WakeValue {
        pub address: super::VirtAddr,
        pub all: bool,
    }
    #[operation(slot = 4, output = u64, infallible)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Clock;

    /// One-time discovery of a directly readable counter on every admitted Hart.
    /// Denied means callers should read the existing Clock operation instead.
    #[operation(slot = 5, output = super::ClockSource, error = RoomFail, reply = 3)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ClockInfo;
}
pub use operations::{RoomCall, RoomFail};

impl crate::abi::reply::FromTriple for ClockSource {
    fn from_triple(_status: usize, boot_tick: usize, hertz: usize) -> Self {
        Self::new(
            boot_tick as u64,
            core::num::NonZeroU64::new(hertz as u64).expect("zero clock frequency"),
        )
    }
}
