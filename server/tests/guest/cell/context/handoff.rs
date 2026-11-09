//! A test-owned rendezvous proves an actual blocked handoff. A short absolute
//! Sleep can already be expired by the time a contended kernel admits it.
use core::sync::atomic::{AtomicU32, Ordering};
use kernel_api::{
    address::VirtAddr,
    model::{
        identity::TaskId,
        unit::{UnitQuery, UnitState, UnitTarget},
        wait::Wait,
    },
};
use kernel_client::call::room;

pub(super) struct Handoff {
    epoch: AtomicU32,
    arrived: AtomicU32,
}
impl Handoff {
    pub(super) const fn new() -> Self {
        Self {
            epoch: AtomicU32::new(0),
            arrived: AtomicU32::new(0),
        }
    }
    fn address(&self) -> VirtAddr {
        VirtAddr::new(self.epoch.as_ptr() as usize)
    }
    pub(super) fn wait(&self, turn: u32) {
        self.arrived.fetch_add(1, Ordering::Release);
        room::wake_value(VirtAddr::new(self.arrived.as_ptr() as usize), true).unwrap();
        while self.epoch.load(Ordering::Acquire) == turn {
            crate::room::wait(&self.epoch, turn, Wait::Infinite);
        }
        assert_eq!(self.epoch.load(Ordering::Acquire), turn + 1);
    }
    pub(super) fn wake(&self, tasks: &[TaskId], turn: u32) {
        // Work between handoffs is not the blocking transition. Park while
        // the workers execute it instead of adding a fifth Yield competitor.
        let expected = tasks.len() as u32 * (turn + 1);
        loop {
            let observed = self.arrived.load(Ordering::Acquire);
            assert!(observed <= expected, "handoff arrival advanced twice");
            if observed == expected {
                break;
            }
            crate::room::wait(&self.arrived, observed, Wait::Infinite);
        }
        loop {
            let waiting = tasks.iter().all(|id| {
                let state =
                    super::lifecycle::inspect(UnitQuery::Target(UnitTarget::Task(*id))).state;
                assert_ne!(state, UnitState::Exited, "context worker exited at {turn}");
                state == UnitState::Waiting
            });
            if waiting {
                break;
            }
            // Ordinary Tasks have no admitted response bound. Let the workers
            // reach WaitValue without adding another busy Yield competitor;
            // the enclosing process watchdog still detects missing progress.
            room::sleep_until(room::clock().saturating_add(1_000_000)).unwrap();
        }
        self.epoch.store(turn + 1, Ordering::Release);
        assert_eq!(room::wake_value(self.address(), true).unwrap(), tasks.len());
    }
}
