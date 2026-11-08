//! Scheduling, deadlines and atomic waits.

use kernel_api::abi::call::room::operations as op;
use kernel_api::{abi::call::room::RoomFail, address::VirtAddr, model::wait::Wait};

typed_calls! {
    infallible fn yield_() -> () = op::Yield;
    infallible fn clock() -> u64 = op::Clock;
    fallible fn sleep_until(at: u64) -> Result<(), erra::Error<RoomFail>> = op::SleepUntil { at };
    fallible fn wait_value(address: VirtAddr, expected: u32, wait: Wait) -> Result<bool, erra::Error<RoomFail>> = op::WaitValue { address, expected, wait };
    fallible fn wake_value(address: VirtAddr, all: bool) -> Result<usize, erra::Error<RoomFail>> = op::WakeValue { address, all };
}

/// Convert once; reusing this deadline across retries does not extend the timeout.
pub fn deadline_after(duration: core::time::Duration) -> Wait {
    if duration.is_zero() {
        return Wait::Poll;
    }
    let nanos = duration.as_nanos().min(u64::MAX as u128) as u64;
    Wait::At(clock().saturating_add(nanos))
}
