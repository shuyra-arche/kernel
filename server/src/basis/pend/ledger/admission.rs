//! Registration capacity, independent of Unit topology and Pend authorization.
//! A charge reserves capacity before publication and survives until cleanup.
use crate::metal::sync::SpinLock;
use alloc::{sync::Arc, vec::Vec};
use kernel_api::{
    abi::call::pend::PendFail,
    model::identity::{TaskId, TeamId},
};

pub(super) const CAPACITY: usize = 1024;
pub(super) const INCOMING: usize = 256;
pub(super) const PER_TASK: usize = 8;
pub(super) const PER_TEAM: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Sender {
    pub(super) task: TaskId,
    pub(super) team: TeamId,
}

struct Usage {
    total: usize,
    incoming: usize,
    senders: Vec<(Sender, usize)>,
}

pub(super) struct Admission {
    owner: TaskId,
    usage: SpinLock<Usage>,
}

impl Admission {
    pub(super) fn new(owner: TaskId) -> Self {
        Self {
            owner,
            usage: SpinLock::new(Usage {
                total: 0,
                incoming: 0,
                senders: Vec::new(),
            }),
        }
    }

    pub(super) fn reserve(self: &Arc<Self>, sender: Option<Sender>) -> Result<Charge, PendFail> {
        let sender = sender.filter(|sender| sender.task != self.owner);
        let mut usage = self.usage.lock();
        if usage.total == CAPACITY {
            return Err(PendFail::OutOfMemory);
        }
        if let Some(sender) = sender {
            let task = usage
                .senders
                .iter()
                .position(|(known, _)| known.task == sender.task);
            let task_count = task.map_or(0, |at| usage.senders[at].1);
            let team_count: usize = usage
                .senders
                .iter()
                .filter(|(known, _)| known.team == sender.team)
                .map(|(_, count)| count)
                .sum();
            if usage.incoming == INCOMING || task_count == PER_TASK || team_count >= PER_TEAM {
                return Err(PendFail::OutOfMemory);
            }
            if let Some(at) = task {
                usage.senders[at].1 += 1;
            } else {
                usage
                    .senders
                    .try_reserve(1)
                    .map_err(|_| PendFail::OutOfMemory)?;
                usage.senders.push((sender, 1));
            }
            usage.incoming += 1;
        }
        usage.total += 1;
        Ok(Charge {
            admission: self.clone(),
            sender,
        })
    }
}

pub(super) struct Charge {
    admission: Arc<Admission>,
    sender: Option<Sender>,
}

impl Drop for Charge {
    fn drop(&mut self) {
        let mut usage = self.admission.usage.lock();
        assert!(usage.total != 0);
        usage.total -= 1;
        if let Some(sender) = self.sender {
            let at = usage
                .senders
                .iter()
                .position(|(known, _)| *known == sender)
                .expect("charged sender");
            usage.senders[at].1 -= 1;
            if usage.senders[at].1 == 0 {
                usage.senders.swap_remove(at);
            }
            usage.incoming -= 1;
        }
    }
}
