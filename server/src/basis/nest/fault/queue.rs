//! Reservation-owned bounded history. A Task retains its event when terminal
//! history is evicted; neither reads nor duplicate faults renew its deadline.
use crate::metal::sync::SpinLock;
use alloc::{sync::Arc, vec::Vec};
use kernel_api::{
    abi::memory::Failure,
    model::nest::{FAULT_CAPACITY, FaultEvent, FaultSequence, MemoGeneration},
};

pub(super) struct Event {
    pub wire: FaultEvent,
    pub deadline: u64,
    progress: SpinLock<Progress>,
}
#[derive(Clone, Copy)]
pub(super) enum Progress {
    Pending { publication: Option<MemoGeneration> },
    Terminal(Result<(), Failure>),
}
impl Event {
    pub fn progress(&self) -> Progress {
        *self.progress.lock()
    }
    pub fn finish(&self, result: Result<(), Failure>) {
        let mut progress = self.progress.lock();
        if matches!(*progress, Progress::Pending { .. }) {
            *progress = Progress::Terminal(result);
        }
    }
}
pub(super) struct Queue {
    events: Vec<Arc<Event>>,
    last: u64,
    evicted: u64,
}
impl Queue {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            last: 0,
            evicted: 0,
        }
    }
    pub fn last(&self) -> u64 {
        self.last
    }
    pub fn accept(
        &mut self,
        mut wire: FaultEvent,
        deadline: u64,
        now: u64,
    ) -> Result<Arc<Event>, Failure> {
        self.expire(now);
        for event in &self.events {
            wire.sequence = event.wire.sequence;
            if wire == event.wire && matches!(event.progress(), Progress::Pending { .. }) {
                return Ok(event.clone());
            }
        }
        let evict = if self.events.len() == FAULT_CAPACITY {
            Some(
                self.events
                    .iter()
                    .position(|event| matches!(event.progress(), Progress::Terminal(_)))
                    .ok_or(Failure::QueueFull)?,
            )
        } else {
            None
        };
        wire.sequence = self
            .last
            .checked_add(1)
            .and_then(FaultSequence::new)
            .ok_or(Failure::VersionExhausted)?;
        self.events
            .try_reserve(1)
            .map_err(|_| Failure::OutOfMemory)?;
        let event = Arc::try_new(Event {
            wire,
            deadline,
            progress: SpinLock::new(Progress::Pending { publication: None }),
        })
        .map_err(|_| Failure::OutOfMemory)?;
        if let Some(at) = evict {
            self.evicted = self.evicted.max(self.events.remove(at).wire.sequence.get());
        }
        self.last = wire.sequence.get();
        self.events.push(event.clone());
        Ok(event)
    }
    pub fn read(
        &self,
        epoch: kernel_api::model::nest::FaultEpoch,
        after: u64,
    ) -> Result<FaultEvent, Failure> {
        if after > self.last {
            return Err(Failure::Invalid);
        }
        if after != 0 && after < self.evicted {
            return Err(Failure::FaultExpired);
        }
        self.events
            .iter()
            .find(|event| event.wire.epoch == epoch && event.wire.sequence.get() > after)
            .map(|event| event.wire)
            .ok_or(Failure::Busy)
    }
    pub fn invalidate(&self, page: Option<usize>, error: Failure) {
        for event in &self.events {
            if page.is_none_or(|page| event.wire.memo_page == page) {
                event.finish(Err(error));
            }
        }
    }
    pub fn publishing(&self, page: usize, before: u64, after: MemoGeneration) {
        for event in &self.events {
            if event.wire.memo_page == page && event.wire.generation.get() == before {
                let mut progress = event.progress.lock();
                if let Progress::Pending { publication } = &mut *progress {
                    *publication = Some(after);
                }
            }
        }
    }
    pub fn expire(&self, now: u64) {
        for event in &self.events {
            if now >= event.deadline {
                event.finish(Err(Failure::TimedOut));
            }
        }
    }
}
