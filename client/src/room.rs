//! Room operations for parking, waiting, waking and reaping a task.

use crate::call;
use core::time::Duration;
use env::{Reason, VirtAddr};

pub use call::room::{wait, wake};

/// Park for at least this duration, rounding fractional milliseconds upward.
pub fn park(duration: Duration) -> env::RoomResult<()> {
    let mut millis = duration.as_millis();
    if duration.subsec_nanos() % 1_000_000 != 0 {
        millis += 1;
    }
    call::room::park(millis.min(usize::MAX as u128) as usize)
}

/// Reap the current task with a reason and an optional diagnostic note.
pub fn reap(reason: Reason, note: Option<&str>) -> ! {
    let note = note.unwrap_or("");
    let call = env::abi::call::room::operations::Reap {
        reason,
        note: VirtAddr::new(note.as_ptr() as usize),
        len: note.len().min(env::NOTE_MAX),
    };
    unsafe { crate::invoke::invoke_terminal(call) }
}
