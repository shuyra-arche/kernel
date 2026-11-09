//! A scheduling turn: immutable observations evaluated inside COMMIT.
//! The caller applies the choice before releasing that same guard, so these
//! observations never become a competing or stale execution authority.
#[derive(Clone, Copy)]
pub(crate) enum Cause {
    Tick,
    Yield,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Continue { spend: bool, arm: bool },
    Rotate,
    Park,
}
pub(crate) struct Turn {
    pub stopped: bool,
    pub ready: bool,
    pub remaining: u32,
    pub now: u64,
    pub ceiling: u64,
}
impl Turn {
    pub(crate) fn decide(self, cause: Cause) -> Choice {
        if self.stopped {
            return Choice::Park;
        }
        match cause {
            Cause::Yield if !self.ready => Choice::Continue {
                spend: false,
                arm: false,
            },
            Cause::Yield => Choice::Rotate,
            Cause::Tick if !self.ready || (self.remaining > 1 && self.now < self.ceiling) => {
                Choice::Continue {
                    spend: self.remaining > 1,
                    arm: true,
                }
            }
            Cause::Tick => Choice::Rotate,
        }
    }
}
