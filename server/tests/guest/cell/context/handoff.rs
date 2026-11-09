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
    entered: [AtomicU32; 2],
}
impl Handoff {
    pub(super) const fn new() -> Self {
        Self {
            epoch: AtomicU32::new(0),
            entered: [const { AtomicU32::new(0) }; 2],
        }
    }
    fn address(&self) -> VirtAddr {
        VirtAddr::new(self.epoch.as_ptr() as usize)
    }
    pub(super) fn wait(&self, id: usize, turn: u32) {
        self.entered[id - 1].store(turn + 1, Ordering::Release);
        while self.epoch.load(Ordering::Acquire) == turn {
            room::wait_value(self.address(), turn, Wait::Infinite).unwrap();
        }
        assert_eq!(self.epoch.load(Ordering::Acquire), turn + 1);
    }
    pub(super) fn wake(&self, tasks: &[TaskId; 2], turn: u32) {
        let deadline = room::clock().saturating_add(3_000_000_000);
        loop {
            let waiting = self
                .entered
                .iter()
                .all(|value| value.load(Ordering::Acquire) == turn + 1)
                && tasks.iter().all(|id| {
                    super::lifecycle::inspect(UnitQuery::Target(UnitTarget::Task(*id))).state
                        == UnitState::Waiting
                });
            if waiting {
                break;
            }
            assert!(
                room::clock() < deadline,
                "context workers did not block at {turn}"
            );
            room::yield_();
        }
        self.epoch.store(turn + 1, Ordering::Release);
        assert_eq!(room::wake_value(self.address(), true).unwrap(), tasks.len());
    }
}
