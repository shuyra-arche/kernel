//! Fixed-capacity attribution of the slowest outer transactions. Equal sites
//! retain only their largest observed hold; this is not a complete site census.
use core::panic::Location;
#[derive(Clone, Copy)]
pub(crate) struct Transaction {
    pub(crate) site: &'static Location<'static>,
    pub(crate) start: u64,
    pub(crate) wait: u64,
}
/// Inclusive stage durations. Stages can nest, so they must not be summed or
/// subtracted from independent maxima to infer an exclusive cost partition.
#[derive(Clone, Copy)]
#[repr(usize)]
pub(crate) enum Work {
    Scheduler,
    SignWait,
    IrqMaintenance,
    ControllerClaim,
    SpinWait,
    TimerArm,
}
pub(crate) const WORKS: usize = 6;
#[derive(Clone, Copy)]
pub(crate) struct Parts {
    pub(crate) ticks: [u64; WORKS],
    pub(crate) calls: [u64; WORKS],
}
impl Parts {
    pub(crate) const fn new() -> Self {
        Self {
            ticks: [0; WORKS],
            calls: [0; WORKS],
        }
    }
    pub(crate) fn record(&mut self, work: Work, ticks: u64) {
        let at = work as usize;
        self.ticks[at] = self.ticks[at].saturating_add(ticks);
        self.calls[at] = self.calls[at].saturating_add(1);
    }
}
#[derive(Clone, Copy)]
pub(super) struct Sample {
    pub(super) site: &'static Location<'static>,
    pub(super) hold: u64,
    pub(super) wait: u64,
    pub(super) transaction: u64,
    pub(super) parts: Parts,
}
pub(super) struct Slowest {
    pub(super) samples: [Option<Sample>; 8],
}
impl Slowest {
    pub(super) const fn new() -> Self {
        Self { samples: [None; 8] }
    }
    pub(super) fn record(&mut self, sample: Sample) {
        let index = self
            .samples
            .iter()
            .position(|entry| entry.is_some_and(|v| v.site == sample.site))
            .or_else(|| self.samples.iter().position(Option::is_none))
            .unwrap_or_else(|| {
                self.samples
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| v.unwrap().hold)
                    .unwrap()
                    .0
            });
        if self.samples[index].is_none_or(|old| sample.hold > old.hold) {
            self.samples[index] = Some(sample);
        }
    }
}
