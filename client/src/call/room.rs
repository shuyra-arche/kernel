//! Scheduling, deadlines and atomic waits.

use kernel_api::abi::call::room::operations as op;
#[cfg(target_arch = "riscv64")]
use kernel_api::model::clock::ClockSource;
use kernel_api::{abi::call::room::RoomFail, address::VirtAddr, model::wait::Wait};

typed_calls! {
    infallible fn yield_() -> () = op::Yield;
    infallible fn clock() -> u64 = op::Clock;
    fallible fn sleep_until(at: u64) -> Result<(), erra::Error<RoomFail>> = op::SleepUntil { at };
    fallible fn wait_value(address: VirtAddr, expected: u32, wait: Wait) -> Result<bool, erra::Error<RoomFail>> = op::WaitValue { address, expected, wait };
    fallible fn wake_value(address: VirtAddr, all: bool) -> Result<usize, erra::Error<RoomFail>> = op::WakeValue { address, all };
}

/// A monotonic clock reader with a cached counter source when the Kernel
/// authorizes it. Construct inside a Kernel Task and reuse for repeated reads.
pub struct MonotonicClock {
    #[cfg(target_arch = "riscv64")]
    source: Option<ClockSource>,
}

impl MonotonicClock {
    pub fn new() -> Self {
        Self {
            #[cfg(target_arch = "riscv64")]
            source: unsafe { crate::invoke::invoke(op::ClockInfo) }.ok(),
        }
    }

    pub fn is_fast(&self) -> bool {
        #[cfg(target_arch = "riscv64")]
        {
            self.source.is_some()
        }
        #[cfg(not(target_arch = "riscv64"))]
        false
    }

    #[inline]
    pub fn now(&self) -> u64 {
        #[cfg(target_arch = "riscv64")]
        if let Some(source) = self.source {
            let tick: u64;
            // ClockInfo guarantees RV64 rdtime access on all admitted Harts.
            unsafe {
                core::arch::asm!("rdtime {}", out(reg) tick, options(nomem, nostack));
            }
            return source.nanos(tick);
        }
        clock()
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert once; reusing this deadline across retries does not extend the timeout.
pub fn deadline_after(duration: core::time::Duration) -> Wait {
    if duration.is_zero() {
        return Wait::Poll;
    }
    let nanos = duration.as_nanos().min(u64::MAX as u128) as u64;
    Wait::At(clock().saturating_add(nanos))
}
