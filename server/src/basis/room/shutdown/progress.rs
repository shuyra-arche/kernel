//! Shutdown progress observations. The owner serializes observation and claim;
//! once Reported, no Hart can obtain a second report from this monitor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Progress {
    Idle,
    Watching { counts: (usize, usize), since: u64 },
    Reported,
}
impl Progress {
    pub(crate) fn observe(&mut self, now: u64, counts: (usize, usize), limit: u64) -> bool {
        if *self == Self::Reported {
            return false;
        }
        if counts.1 == 0 || counts.1 >= counts.0 {
            *self = Self::Idle;
            return false;
        }
        match *self {
            Self::Watching {
                counts: previous,
                since,
            } if previous == counts => {
                if now.saturating_sub(since) < limit {
                    return false;
                }
                *self = Self::Reported;
                true
            }
            _ => {
                *self = Self::Watching { counts, since: now };
                false
            }
        }
    }
}
