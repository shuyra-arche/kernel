extern crate alloc;
#[path = "../../src/basis/room/deadline/queue.rs"]
mod queue;
mod timer;
use queue::Queue;
use std::collections::BTreeMap;

#[test]
fn cancelled_or_replaced_deadlines_never_expire_twice() {
    let mut queue = Queue::new();
    queue.insert(1, 3).unwrap();
    queue.insert(2, 2).unwrap();
    queue.insert(1, 7).unwrap();
    assert!(queue.remove(2));
    assert!(!queue.remove(2));
    assert_eq!(queue.nearest(), Some(7));
    assert_eq!(queue.pop_due(6), None);
    assert_eq!(queue.pop_due(7), Some((7, 1)));
    assert_eq!(queue.pop_due(u64::MAX), None);
    queue.insert(3, u64::MAX).unwrap();
    assert_eq!(queue.pop_due(u64::MAX - 1), None);
    assert_eq!(queue.pop_due(u64::MAX), Some((u64::MAX, 3)));
}
#[test]
fn mixed_deadline_operations_match_an_independent_ordered_model() {
    let mut queue = Queue::new();
    let mut model = BTreeMap::new();
    let mut random = 0x1234_5678u64;
    for _ in 0..20_000 {
        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
        let ticket = random >> 32 & 255;
        let time = random >> 16 & 1023;
        match random % 3 {
            0 => {
                queue.insert(ticket, time).unwrap();
                model.insert(ticket, time);
            }
            1 => {
                assert_eq!(queue.remove(ticket), model.remove(&ticket).is_some());
            }
            _ => {
                let expected = model
                    .iter()
                    .map(|(&id, &at)| (at, id))
                    .min()
                    .filter(|e| e.0 <= time);
                assert_eq!(queue.pop_due(time), expected);
                if let Some((_, id)) = expected {
                    model.remove(&id);
                }
            }
        }
        assert_eq!(queue.nearest(), model.values().copied().min());
    }
}

#[path = "../../src/metal/generic/clock.rs"]
pub(crate) mod clock_contract;

mod metal {
    pub(crate) mod generic {
        pub(crate) use crate::clock_contract as clock;
    }
}

mod membership;
use membership::Task;
#[path = "../../src/basis/unit/task/chain.rs"]
mod chain;
mod scheduling;
mod shutdown;
