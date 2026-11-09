//! An admitted fixed-priority CPU reservation. Times are nanoseconds.
//!
//! `latency` is the administrator's bound on cumulative platform/kernel
//! interference during one response window, not a measured hardware guarantee.
//! Admission is conditional on that assumption and on resident, restricted work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Schedule {
    pub hart: u64,
    pub priority: u64,
    pub ceiling: u64,
    pub budget: u64,
    pub period: u64,
    pub latency: u64,
    /// Bit 0 prepares F/D; bit 1 prepares V (including F/D).
    pub extensions: u64,
}

impl Schedule {
    pub const WIDTH: usize = 56;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut out = [0; Self::WIDTH];
        for (chunk, value) in out.chunks_exact_mut(8).zip([
            self.hart,
            self.priority,
            self.ceiling,
            self.budget,
            self.period,
            self.latency,
            self.extensions,
        ]) {
            chunk.copy_from_slice(&value.to_le_bytes());
        }
        out
    }
    pub fn from_bytes(bytes: &[u8; Self::WIDTH]) -> Option<Self> {
        let word = |at| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
        let value = Self {
            hart: word(0),
            priority: word(8),
            ceiling: word(16),
            budget: word(24),
            period: word(32),
            latency: word(40),
            extensions: word(48),
        };
        (value.hart != 0
            && value.priority != 0
            && value.priority <= value.ceiling
            && value.ceiling <= 31
            && value.budget >= 100_000
            && value.budget < value.period
            && (1_000_000..=10_000_000_000).contains(&value.period)
            && value.latency != 0
            && value.latency < value.period
            && value.extensions & !3 == 0)
            .then_some(value)
    }
}

/// A coherent observation; it is neither a second budget nor authorization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduleInfo {
    pub schedule: Schedule,
    pub effective: u64,
    pub remaining: u64,
    pub replenishment: u64,
    pub consumed: u64,
    pub overruns: u64,
    pub response: u64,
}
impl ScheduleInfo {
    pub const WIDTH: usize = Schedule::WIDTH + 48;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut out = [0; Self::WIDTH];
        out[..Schedule::WIDTH].copy_from_slice(&self.schedule.to_bytes());
        for (chunk, value) in out[Schedule::WIDTH..].chunks_exact_mut(8).zip([
            self.effective,
            self.remaining,
            self.replenishment,
            self.consumed,
            self.overruns,
            self.response,
        ]) {
            chunk.copy_from_slice(&value.to_le_bytes());
        }
        out
    }
    pub fn from_bytes(bytes: &[u8; Self::WIDTH]) -> Option<Self> {
        let word = |at| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
        Some(Self {
            schedule: Schedule::from_bytes(bytes[..Schedule::WIDTH].try_into().ok()?)?,
            effective: word(56),
            remaining: word(64),
            replenishment: word(72),
            consumed: word(80),
            overruns: word(88),
            response: word(96),
        })
    }
}
