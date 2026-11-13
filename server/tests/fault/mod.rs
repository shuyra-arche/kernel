#![feature(allocator_ext)]
extern crate alloc;
// Production queue and event lifetime; Task authority, scheduling and MMU
// publication are deliberately outside this host fixture.
#[path = "../name/allocation.rs"]
mod allocation;
#[path = "../support/lock.rs"]
mod lock;
#[path = "../../src/basis/nest/fault/queue.rs"]
mod queue;
mod metal {
    pub(crate) use crate::lock as sync;
}
use kernel_api::{
    abi::memory::Failure,
    model::{
        flat::{FlatId, SlotVersion},
        nest::{
            FAULT_CAPACITY, FaultAccess, FaultCause, FaultEpoch, FaultEvent, FaultSequence,
            MemoGeneration, NestId, ReservationId,
        },
    },
};
use queue::{Progress, Queue};
fn event(page: usize, epoch: u64) -> FaultEvent {
    FaultEvent {
        nest: NestId::new(1).unwrap(),
        reservation: ReservationId::from_bytes(&2u64.to_le_bytes()).unwrap(),
        epoch: FaultEpoch::new(epoch).unwrap(),
        sequence: FaultSequence::new(1).unwrap(),
        memo_page: page,
        generation: MemoGeneration::INITIAL,
        flat: FlatId::new(3).unwrap(),
        slot_page: page + 512,
        version: SlotVersion::INITIAL,
        cause: FaultCause::Missing,
        access: FaultAccess::Read,
    }
}
#[test]
fn duplicates_and_reads_retain_the_first_deadline_and_do_not_consume() {
    let mut queue = Queue::new();
    let first = queue.accept(event(0, 2), 100, 1).unwrap();
    let duplicate = queue.accept(event(0, 2), 150, 50).unwrap();
    assert!(std::sync::Arc::ptr_eq(&first, &duplicate));
    assert_eq!(duplicate.deadline, 100);
    for _ in 0..3 {
        assert_eq!(
            queue.read(FaultEpoch::new(2).unwrap(), 0).unwrap(),
            first.wire
        );
    }
    assert_eq!(queue.read(first.wire.epoch, 1), Err(Failure::Busy));
    queue.expire(100);
    assert!(matches!(
        first.progress(),
        Progress::Terminal(Err(Failure::TimedOut))
    ));
    queue.publishing(0, 1, MemoGeneration::new(2).unwrap());
    first.finish(Ok(()));
    assert!(matches!(
        first.progress(),
        Progress::Terminal(Err(Failure::TimedOut))
    ));
}
#[test]
fn pending_capacity_never_evicts_and_retained_waiters_survive_terminal_eviction() {
    let mut queue = Queue::new();
    let events: Vec<_> = (0..FAULT_CAPACITY)
        .map(|page| queue.accept(event(page, 2), 100, 0).unwrap())
        .collect();
    assert!(matches!(
        queue.accept(event(17, 2), 100, 0),
        Err(Failure::QueueFull)
    ));
    assert_eq!(queue.last(), 16);
    events[1].finish(Err(Failure::Stale));
    let next = queue.accept(event(17, 2), 100, 0).unwrap();
    assert_eq!(next.wire.sequence.get(), 17);
    assert!(matches!(
        events[1].progress(),
        Progress::Terminal(Err(Failure::Stale))
    ));
    assert_eq!(queue.read(next.wire.epoch, 1), Err(Failure::FaultExpired));
    assert_eq!(queue.read(next.wire.epoch, 0).unwrap(), events[0].wire);
    assert_eq!(queue.read(next.wire.epoch, 18), Err(Failure::Invalid));
}
#[test]
fn replacement_keeps_sequence_monotonic_and_cannot_retarget_old_events() {
    let mut queue = Queue::new();
    let first = queue.accept(event(0, 2), 100, 0).unwrap();
    let other = queue.accept(event(1, 2), 100, 0).unwrap();
    queue.publishing(0, 9, MemoGeneration::new(10).unwrap());
    assert!(matches!(
        first.progress(),
        Progress::Pending { publication: None }
    ));
    queue.publishing(0, 1, MemoGeneration::new(2).unwrap());
    assert!(matches!(first.progress(), Progress::Pending { publication: Some(g) } if g.get() == 2));
    assert!(matches!(
        other.progress(),
        Progress::Pending { publication: None }
    ));
    queue.invalidate(Some(0), Failure::Stale);
    assert!(matches!(
        first.progress(),
        Progress::Terminal(Err(Failure::Stale))
    ));
    queue.invalidate(None, Failure::HandlerGone);
    let replacement = queue.accept(event(0, 3), 200, 0).unwrap();
    assert_eq!(replacement.wire.sequence.get(), 3);
    assert_eq!(
        queue.read(replacement.wire.epoch, 0).unwrap(),
        replacement.wire
    );
    assert!(matches!(
        other.progress(),
        Progress::Terminal(Err(Failure::HandlerGone))
    ));
}
#[test]
fn failed_admission_does_not_consume_sequence_or_evict_history() {
    let mut queue = Queue::new();
    for budget in 0..2 {
        assert!(matches!(
            allocation::fail_after(budget, || queue.accept(event(0, 2), 10, 0)),
            Err(Failure::OutOfMemory)
        ));
        assert_eq!(queue.last(), 0);
    }
    assert_eq!(
        queue
            .accept(event(0, 2), 10, 0)
            .unwrap()
            .wire
            .sequence
            .get(),
        1
    );
}
