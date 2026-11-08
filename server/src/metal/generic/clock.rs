//! Counter settings and the comparator programming contract.

pub(crate) const DISARMED: u64 = u64::MAX;

/// Absolute ticks in the kernel clock's counter domain. The counter horizon is off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Compare(u64);

impl Compare {
    pub(crate) const OFF: Self = Self(DISARMED);

    pub(crate) const fn from_ticks(ticks: u64) -> Self {
        Self(ticks)
    }

    pub(crate) const fn ticks(self) -> u64 {
        self.0
    }
}

pub(crate) trait Comparator {
    type Error;

    /// Program the current Hart's comparator while interrupts are masked.
    /// Success installs the setting. An error may leave the hardware changed.
    fn program(&self, setting: Compare) -> Result<(), Self::Error>;
}
