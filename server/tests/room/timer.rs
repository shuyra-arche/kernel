#[path = "../../src/metal/clock/timer/local.rs"]
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
