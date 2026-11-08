//! Shared RV64 monotonic clock epoch and scale.
use core::num::NonZeroU64;

const NANOS_PER_SEC: u128 = 1_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockSource {
    boot_tick: u64,
    hertz: NonZeroU64,
}

impl ClockSource {
    pub const fn new(boot_tick: u64, hertz: NonZeroU64) -> Self {
        Self { boot_tick, hertz }
    }

    pub const fn boot_tick(self) -> u64 {
        self.boot_tick
    }

    pub const fn hertz(self) -> NonZeroU64 {
        self.hertz
    }

    /// Floor an observed hardware tick to absolute nanoseconds since boot.
    pub fn nanos(self, tick: u64) -> u64 {
        self.ticks_to_nanos(tick.saturating_sub(self.boot_tick))
    }

    pub fn ticks_to_nanos(self, ticks: u64) -> u64 {
        ((ticks as u128 * NANOS_PER_SEC) / self.hertz.get() as u128).min(u64::MAX as u128) as u64
    }

    /// Round a requested duration up so the corresponding wait cannot end early.
    pub fn duration_ticks(self, nanos: u128) -> u64 {
        nanos
            .saturating_mul(self.hertz.get() as u128)
            .div_ceil(NANOS_PER_SEC)
            .min(u64::MAX as u128) as u64
    }

    pub fn deadline(self, nanos: u64) -> u64 {
        self.boot_tick
            .saturating_add(self.duration_ticks(nanos as u128))
    }
}
