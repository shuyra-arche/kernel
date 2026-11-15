#[path = "../../src/basis/nest/translation/tickets.rs"]
mod tickets;

use crate::scope::{Shootdown, invalidate};
use std::cell::Cell;
use tickets::Tickets;

struct ScriptedFence(Cell<bool>);

// This fixture has no hardware translations. It can reject or confirm the
// requested target so the test can distinguish the two completion paths.
unsafe impl Shootdown for ScriptedFence {
    type Target = usize;
    type Error = ();

    fn invalidate(&self, target: &usize) -> Result<(), ()> {
        assert_eq!(*target, 7);
        self.0.get().then_some(()).ok_or(())
    }
}

#[test]
fn reclaiming_a_fenced_ticket_keeps_later_failed_tickets_reserved() {
    let backend = ScriptedFence(Cell::new(true));
    let mut tickets = Tickets::new();
    tickets.reserve(3).unwrap();
    tickets.retire(1);
    tickets.retire(2);
    assert!(invalidate(&backend, 7).unwrap().confirms(7));
    tickets.settle();

    tickets.retire(3);
    backend.0.set(false);
    assert!(invalidate(&backend, 7).is_err());
    assert!(!tickets.claim(3));
    assert!(tickets.claim(1));
    assert!(tickets.has_settled());
    assert_eq!(tickets.drain(4).collect::<Vec<_>>(), [2]);
    assert!(!tickets.has_settled());

    backend.0.set(true);
    assert!(invalidate(&backend, 7).unwrap().confirms(7));
    tickets.settle();
    assert_eq!(tickets.drain(4).collect::<Vec<_>>(), [3]);
}

#[test]
fn cpu_completion_is_bound_to_domain_attempt_and_full_coverage() {
    use crate::scope::{Request, Scope};
    struct Cpu;
    // Host fixture owns no actual translations; it reports only this invocation.
    unsafe impl Shootdown for Cpu {
        type Target = Request<usize>;
        type Error = ();
        fn invalidate(&self, _: &Self::Target) -> Result<(), ()> {
            Ok(())
        }
    }
    let pending = Request {
        domain: 3,
        attempt: 7,
        identifier: 4,
        scope: Scope::Global,
    };
    for unrelated in [
        Request {
            domain: 2,
            ..pending
        },
        Request {
            attempt: 6,
            ..pending
        },
        Request {
            identifier: 5,
            ..pending
        },
        Request {
            scope: Scope::AddressSpace,
            ..pending
        },
    ] {
        assert!(!invalidate(&Cpu, unrelated).unwrap().confirms(pending));
    }
    assert!(invalidate(&Cpu, pending).unwrap().confirms(pending));
}

#[test]
fn ticket_batches_retain_the_unprocessed_and_unfenced_suffixes() {
    let mut tickets = Tickets::new();
    tickets.reserve(12).unwrap();
    for id in 1..=9 {
        tickets.retire(id);
    }
    tickets.settle();
    tickets.retire(10);
    assert_eq!(tickets.drain(4).collect::<Vec<_>>(), [1, 2, 3, 4]);
    assert_eq!(tickets.drain(4).collect::<Vec<_>>(), [5, 6, 7, 8]);
    assert_eq!(tickets.drain(4).collect::<Vec<_>>(), [9]);
    assert!(!tickets.has_settled());
    assert!(!tickets.claim(10));
    tickets.settle();
    assert_eq!(tickets.drain(4).collect::<Vec<_>>(), [10]);
}
