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
fn concurrent_observers_cannot_replace_the_initiators_exact_request() {
    let pending = Mutex::new(Retirement::new());
    pending.lock().unwrap().mark_dirty(true);
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
    assert_eq!(targets.iter().filter(|target| target.is_ok()).count(), 1);
    assert_eq!(
        targets
            .iter()
            .filter(|target| matches!(target, Err(NestError::Busy)))
            .count(),
        7
    );
    let target = targets.into_iter().find_map(Result::ok).unwrap();
    assert_eq!(target.scope, Scope::Global);
    let mut pending = pending.lock().unwrap();
    pending
        .complete(scope::invalidate(&Cpu, target).unwrap(), &mut Memos)
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
            pending.complete(scope::invalidate(&Cpu, wrong).unwrap(), &mut Memos),
            Err(NestError::Shootdown)
        ));
        assert_eq!(pool.gives(), 0);
        assert!(!pending.tickets.claim(11));
        let retry = pending.begin(7, 3).unwrap();
        let fenced = pending
            .complete(scope::invalidate(&Cpu, retry).unwrap(), &mut Memos)
            .unwrap();
        assert!(pending.tickets.claim(11));
        assert_eq!(pool.gives(), 0); // Release happens outside the Nest lock.
        let nest = Nest {
            state: crate::host_lock::SpinLock::new(RetirementState { memos: Memos }),
        };
        fenced.release(&nest);
        assert_eq!(pool.gives(), 1);
    }
}
