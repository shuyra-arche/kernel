#[path = "../../src/metal/riscv/clock/timer/local.rs"]
mod local;

use crate::metal::generic::clock::{Comparator, Compare};
use local::LocalTimer;
use std::cell::Cell;

struct ScriptedComparator {
    hardware: Cell<Compare>,
    fail: Cell<bool>,
}

impl Comparator for &ScriptedComparator {
    type Error = ();

    fn program(&self, setting: Compare) -> Result<(), Self::Error> {
        self.hardware.set(setting);
        if self.fail.replace(false) {
            return Err(());
        }
        Ok(())
    }
}

#[test]
fn failed_comparator_write_invalidates_cached_setting() {
    let first = Compare::from_ticks(100);
    let next = Compare::from_ticks(200);
    let backend = ScriptedComparator {
        hardware: Cell::new(first),
        fail: Cell::new(false),
    };
    let timer = LocalTimer::new(&backend).unwrap();
    assert_eq!(backend.hardware.get(), Compare::OFF);
    assert_eq!(timer.set(Compare::OFF), Ok(false));
    assert_eq!(timer.set(first), Ok(true));

    backend.fail.set(true);
    assert_eq!(timer.set(next), Err(()));
    assert_eq!(backend.hardware.get(), next);
    assert_eq!(timer.setting(), None);

    assert_eq!(timer.set(first), Ok(true));
    assert_eq!(backend.hardware.get(), first);
    assert_eq!(timer.setting(), Some(first));
    assert_eq!(timer.set(first), Ok(false));
}

#[test]
fn retry_backs_off_due_interrupts_without_postponing_an_earlier_deadline() {
    let backend = ScriptedComparator {
        hardware: Cell::new(Compare::OFF),
        fail: Cell::new(false),
    };
    let timer = LocalTimer::new(&backend).unwrap();
    timer.retry(100, 200).unwrap();
    assert_eq!(backend.hardware.get().ticks(), 200);
    timer.set(Compare::from_ticks(150)).unwrap();
    assert_eq!(timer.retry(100, 200), Ok(false));
    assert_eq!(backend.hardware.get().ticks(), 150);
    timer.retry(150, 250).unwrap();
    assert_eq!(backend.hardware.get().ticks(), 250);
    // Failed hardware writes invalidate the cache: a subsequent retry must
    // program hardware even if it matches the last successful setting.
    backend.fail.set(true);
    assert_eq!(timer.set(Compare::from_ticks(300)), Err(()));
    assert_eq!(timer.retry(200, 250), Ok(true));
    assert_eq!(backend.hardware.get().ticks(), 250);
}

#[path = "../../src/basis/room/deadline/retry.rs"]
mod retry;

#[test]
fn retry_is_retained_until_due_and_contention_does_not_create_a_past_comparator() {
    use crate::metal::generic::clock::DISARMED;
    let retry = retry::Retry::new();
    assert_eq!(retry.deadline(), DISARMED);
    assert_eq!(retry.request(0, 10), 10);
    assert_eq!(retry.request(2, 20), 10);
    retry.elapsed(5);
    assert_eq!(retry.deadline(), 10);
    assert_eq!(retry.request(10, 10), 20);
    retry.elapsed(20);
    assert_eq!(retry.deadline(), DISARMED);
    assert_eq!(retry.request(20, 0), 21);
    retry.elapsed(21);
    assert_eq!(retry.request(DISARMED - 10, 20), DISARMED - 1);
}
