#[path = "../../../../../src/basis/nest/memo/activity/epoch.rs"]
mod epoch;
#[test]
fn epochs_retain_capture_through_retry_and_never_reveal_preset_facts() {
    use epoch::{History, Phase};
    let mut h = History::<u64>::new();
    h.capture((true, true), 10);
    assert_eq!(h.snapshot().epoch, 0);
    if let Phase::Pending { round, .. } = &mut h.phase {
        *round = Some(7);
    } else {
        panic!("pending round");
    }
    // Failed/unacknowledged polling performs no state transition and no clear.
    assert_eq!(h.snapshot().epoch, 0);
    h.complete(20);
    assert!(matches!(
        h.phase,
        Phase::Armed {
            synchronized: 20,
            ..
        }
    ));
    assert_eq!((h.snapshot().accessed, h.snapshot().dirty), (0, 0));
    h.capture((true, false), 30);
    h.complete(40);
    let first = h.snapshot();
    assert_eq!((first.epoch, first.accessed, first.dirty), (2, 2, 0));
    h.capture((false, true), 50);
    h.complete(60);
    let next = h.snapshot();
    assert_eq!((next.accessed, next.dirty), (2, 3));
    // Two recipient cursors cannot erase another reader's cumulative facts.
    assert!(next.accessed > 1 && next.dirty > 1);
    assert!(next.accessed <= 2 && next.dirty > 2);
    h.snapshot.epoch = u64::MAX;
    h.capture((true, true), 70);
    h.complete(80);
    assert!(matches!(h.phase, Phase::Exhausted));
    assert_eq!(h.snapshot().epoch, u64::MAX);
}
