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
    assert_eq!(invalidate(&backend, 7).unwrap().target(), 7);
    tickets.settle();

    tickets.retire(3);
    backend.0.set(false);
    assert!(invalidate(&backend, 7).is_err());
    assert!(!tickets.claim(3));
    assert!(tickets.claim(1));
    assert!(tickets.has_settled());
    assert_eq!(tickets.drain().collect::<Vec<_>>(), [2]);
    assert!(!tickets.has_settled());

    backend.0.set(true);
    assert_eq!(invalidate(&backend, 7).unwrap().target(), 7);
    tickets.settle();
    assert_eq!(tickets.drain().collect::<Vec<_>>(), [3]);
}
