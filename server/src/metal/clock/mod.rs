mod error;
mod instant;
mod riscv;
pub(crate) mod timer;

pub use error::ClockError;
pub use instant::Instant;

use ::core::num::NonZeroU64;
use ::core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use ::core::time::Duration;

use kernel_api::model::clock::ClockSource;

use self::riscv as counter;
use crate::metal::board;
use crate::metal::sync::OnceLock;

const NANOS_PER_SEC: u128 = 1_000_000_000;

static CLOCK: OnceLock<ClockSource> = OnceLock::new();
static USER_TIME: AtomicBool = AtomicBool::new(false);
static USER_TIME_HARTS: AtomicUsize = AtomicUsize::new(0);

pub fn init() -> Result<(), ClockError> {
    let hertz = NonZeroU64::new(board::info().hart.hertz as u64).ok_or(ClockError::NoTimebase)?;
    CLOCK
        .set(ClockSource::new(counter::ticks(), hertz))
        .map_err(|_| ClockError::AlreadyInit)?;
    timer::configure(firmware_allows("shuyra,supervisor-stimecmp"));
    USER_TIME.store(firmware_allows("shuyra,user-time"), Ordering::Release);
    Ok(())
}

// These opt-ins come from the trusted boot DT, never from a Task or ISA alone.
fn firmware_allows(name: &str) -> bool {
    let Some(tree) = board::device_tree() else {
        return false;
    };
    let Some(chosen) = tree.find_node("/chosen") else {
        return false;
    };
    matches!(board::tree::property(chosen, name), Ok(Some([])))
}

pub(crate) fn initialize_hart() {
    timer::initialize();
    if USER_TIME.load(Ordering::Acquire) && counter::enable_user_time() {
        USER_TIME_HARTS.fetch_add(1, Ordering::Release);
    }
}

pub(crate) fn user_source() -> Option<ClockSource> {
    (USER_TIME_HARTS.load(Ordering::Acquire) == crate::metal::hart::hart_count()).then(source)
}

fn source() -> ClockSource {
    *CLOCK.get().expect("clock not initialized")
}

pub fn now() -> Instant {
    Instant::from_ticks(counter::ticks())
}

pub(crate) fn uptime_ticks() -> u64 {
    (counter::ticks()).saturating_sub(source().boot_tick())
}

pub fn uptime() -> Duration {
    Duration::from_nanos(source().nanos(now().as_ticks()))
}

pub(crate) fn ticks_to_duration(ticks: u64) -> Duration {
    Duration::from_nanos(source().ticks_to_nanos(ticks))
}

pub(crate) fn duration_to_ticks(d: Duration) -> u64 {
    source().duration_ticks(d.as_nanos())
}

/// Convert an absolute ABI deadline once, rounding up to prevent early wakeups.
pub(crate) fn deadline(wait: kernel_api::model::wait::Wait) -> Option<u64> {
    use kernel_api::model::wait::Wait;
    match wait {
        Wait::Infinite => None,
        Wait::Poll => Some(now().as_ticks()),
        Wait::At(ns) => Some(source().deadline(ns)),
    }
}
pub(crate) fn remaining(at: Option<u64>) -> Duration {
    at.map_or(Duration::MAX, |at| {
        Duration::from_nanos(
            ((at.saturating_sub(now().as_ticks()) as u128 * NANOS_PER_SEC)
                .div_ceil(source().hertz().get() as u128))
            .min(u64::MAX as u128) as u64,
        )
    })
}
