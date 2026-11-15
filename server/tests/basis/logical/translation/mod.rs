//! Production CPU retirement with inert index/Span/table owners. Scripted
//! completions establish bookkeeping only; actual shootdown needs QEMU/RV64.
#[path = "../../../../src/basis/nest/translation/invalidation.rs"]
mod invalidation;
use invalidation::Invalidation;
#[path = "../../../../src/basis/nest/translation/retirement.rs"]
mod retirement;
#[path = "../../../../src/basis/nest/translation/tickets.rs"]
mod tickets;

use super::{Nest, NestError, RetirementState, memo::Memos};
use crate::{
    cell::{Owner, Page},
    memo::Publication,
    scope::{self, Request, Scope, Shootdown},
    support::Pool,
};
use retirement::Retirement;
use std::sync::{Arc, Barrier, Mutex};

struct Cpu;
// No physical translations are installed in this harness.
unsafe impl Shootdown for Cpu {
    type Target = Request<usize>;
    type Error = ();
    fn invalidate(&self, _: &Self::Target) -> Result<(), ()> {
        Ok(())
    }
}

#[test]
fn memo_finishes_with_its_batch_before_a_new_batch_can_delay_resource_release() {
    use super::memo::source::{Registry, Source};
    use std::sync::atomic::AtomicUsize;
    let span = Arc::new(super::Span::new(9));
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let mut memos = Memos::new();
    let memo = memos.get_mut(&span, 0).unwrap();
    memo.associate(
        1,
        None,
        Source(Arc::new(Registry(AtomicUsize::new(0)))),
        Arc::new(super::authority::RangeBinding),
    )
    .release();
    let change = page.begin().unwrap();
    memo.begin_publish(memo.generation(), change.clone(), 6)
        .unwrap();
    let mut publication = Publication::prepare(&page, true, Some(&change)).unwrap();
    publication.publish();
    memo.install(publication);
    // Another position accepted Publish, but has not installed its PTE. Even
    // though it shares this Span, this fence does not complete its operation.
    let neighbor_pool = Pool::new(0);
    let neighbor =
        Arc::new(Page::from_internal(Owner::Kernel, neighbor_pool.page().unwrap()).unwrap());
    let memo = memos.get_mut(&span, 1).unwrap();
    memo.associate(
        1,
        None,
        Source(Arc::new(Registry(AtomicUsize::new(0)))),
        Arc::new(super::authority::RangeBinding),
    )
    .release();
    memo.begin_publish(memo.generation(), neighbor.begin().unwrap(), 6)
        .unwrap();
    // Sparse, untouched coordinates must not grow the release allocation for
    // a single edited PTE. Its capacity follows actual hardware obligations.
    for position in 2..1024 {
        memos.get_mut(&span, position).unwrap();
    }
    assert_eq!(memos.awaiting_cpu(&span), 1);
    let mut pending = Retirement::new();
    pending.mappings.push(span.clone());
    assert!(pending.pending_for(&span));
    pending.mark_dirty(false);
    let request = pending.begin(7, 3).unwrap();
    let fenced = pending
        .complete(scope::invalidate(&Cpu, request).unwrap(), &mut memos)
        .unwrap();
    assert!(!memos.get(&span, 0).unwrap().in_flight());
    assert!(memos.get(&span, 1).unwrap().in_flight());
    assert_eq!(memos.awaiting_cpu(&span), 0);
    assert!(!pending.pending_for(&span));
    let nest = Nest {
        state: crate::host_lock::SpinLock::new(RetirementState { memos }),
    };
    fenced.release(&nest);
    assert!(nest.state.lock().memos.get(&span, 1).unwrap().in_flight());
    drop(change);

    let mut state = nest.state.lock();
    let memo = state.memos.existing_mut(&span, 0).unwrap();
    memo.begin_retract(memo.generation(), page.begin().unwrap(), false)
        .unwrap();
    pending.cells.push(memo.withdraw().unwrap());
    pending.mappings.push(span.clone());
    pending.mark_dirty(false);
    let request = pending.begin(7, 3).unwrap();
    let fenced = pending
        .complete(scope::invalidate(&Cpu, request).unwrap(), &mut state.memos)
        .unwrap();
    assert!(!state.memos.get(&span, 0).unwrap().busy());
    // A different PTE batch starts before callbacks and Page drops of the old
    // batch. Its unfinished fence cannot strand the old Memo's activity.
    let unrelated = Arc::new(super::Span::new(10));
    pending.mappings.push(unrelated.clone());
    pending.mark_dirty(false);
    pending.begin(7, 3).unwrap();
    assert!(pending.pending_for(&unrelated));
    assert!(!pending.pending_for(&span));
    drop(state);
    drop(page);
    assert_eq!(pool.gives(), 0);
    fenced.release(&nest);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn editing_owner_claims_its_batch_before_concurrent_retry_observers() {
    let pending = Mutex::new(Retirement::new());
    let original = {
        let mut pending = pending.lock().unwrap();
        pending.mark_dirty(true);
        pending.begin(7, 3).unwrap()
    };
    let barrier = Barrier::new(9);
    let targets = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    pending.lock().unwrap().begin(7, 3)
                })
            })
            .collect();
        barrier.wait();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(targets.iter().all(Result::is_err));
    assert_eq!(
        targets
            .iter()
            .filter(|target| matches!(target, Err(NestError::Busy)))
            .count(),
        8
    );
    assert_eq!(original.scope, Scope::Global);
    let mut pending = pending.lock().unwrap();
    pending
        .complete(
            scope::invalidate(&Cpu, original).unwrap(),
            &mut Memos::new(),
        )
        .unwrap();
    assert_eq!(pending.invalidation(), Invalidation::Clean);
    assert!(matches!(pending.begin(7, 3), Err(NestError::Busy)));
}

#[test]
fn failed_or_unrelated_proofs_keep_page_and_address_responsibility_until_exact_retry() {
    for field in 0..4 {
        let pool = Pool::new(0);
        let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
        let mut publication = Publication::prepare(&page, false, None).unwrap();
        publication.publish();
        let mut pending = Retirement::new();
        pending.cells.push(publication.retire());
        pending.tickets.reserve(1).unwrap();
        pending.tickets.retire(11);
        pending.mark_dirty(true);
        drop(page); // Source exit cannot discharge its hardware responsibility.
        let old = pending.begin(7, 3).unwrap();
        pending.fail(); // SBI/CPU failure keeps the entire batch.
        assert_eq!(pool.gives(), 0);
        assert!(!pending.tickets.claim(11));
        let retry = pending.begin(7, 3).unwrap();
        assert_ne!(retry.attempt, old.attempt);
        let wrong = match field {
            0 => Request { domain: 8, ..retry },
            1 => old,
            2 => Request {
                identifier: 4,
                ..retry
            },
            _ => Request {
                scope: Scope::AddressSpace,
                ..retry
            },
        };
        assert!(matches!(
            pending.complete(scope::invalidate(&Cpu, wrong).unwrap(), &mut Memos::new()),
            Err(NestError::Shootdown)
        ));
        assert_eq!(pool.gives(), 0);
        assert!(!pending.tickets.claim(11));
        let retry = pending.begin(7, 3).unwrap();
        let fenced = pending
            .complete(scope::invalidate(&Cpu, retry).unwrap(), &mut Memos::new())
            .unwrap();
        assert!(pending.tickets.claim(11));
        assert_eq!(pool.gives(), 0); // Release happens outside the Nest lock.
        let nest = Nest {
            state: crate::host_lock::SpinLock::new(RetirementState {
                memos: Memos::new(),
            }),
        };
        fenced.release(&nest);
        assert_eq!(pool.gives(), 1);
    }
}

#[test]
fn retained_nonblocking_retirement_rejects_another_request_and_preserves_one_initiator() {
    let mut retirement = Retirement::new();
    retirement.mark_dirty(false);
    let exact = retirement.begin(7, 3).unwrap();
    let wrong = Request {
        attempt: exact.attempt + 1,
        ..exact
    };
    assert_eq!(
        retirement.park(crate::metal::mmu::Deferred::new(wrong)),
        Err(NestError::Shootdown)
    );
    assert!(retirement.deferred().is_none());
    retirement
        .park(crate::metal::mmu::Deferred::new(exact))
        .unwrap();
    assert!(retirement.deferred().unwrap().confirms(exact));
    assert_eq!(retirement.begin(7, 3), Err(NestError::Busy));
    retirement.fail();
    assert!(retirement.deferred().is_none());
    let retry = retirement.begin(7, 3).unwrap();
    assert!(retry.attempt > exact.attempt);
    let proof = scope::invalidate(&Cpu, retry).unwrap();
    retirement.complete(proof, &mut Memos::new()).unwrap();
    assert!(retirement.invalidation().is_clean());
}
