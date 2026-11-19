//! Test-only real-Hart withholding. Added as a completion child in a disposable
//! provider. The exact production history and Pending round run on real Harts.
use super::*;
use core::sync::atomic::{AtomicUsize, Ordering::*};
#[path = "../../../../../../../src/basis/nest/memo/activity/epoch.rs"]
mod epoch;
static ARRIVED: AtomicUsize = AtomicUsize::new(0);
static PHASE: AtomicUsize = AtomicUsize::new(0);
static HELD: AtomicUsize = AtomicUsize::new(0);
fn now() -> u64 {
    crate::metal::clock::uptime().as_nanos() as u64
}
fn until(mut ready: impl FnMut() -> bool) {
    let end = now() + 10_000_000_000;
    while !ready() {
        assert!(now() < end, "activity fence probe timed out");
        core::hint::spin_loop();
    }
}
fn capture(h: &mut epoch::History<Pending>, bits: (bool, bool)) {
    h.capture(bits, now());
    let epoch::Phase::Pending { round, .. } = &mut h.phase else {
        panic!("pending")
    };
    *round = Some(Pending::begin().unwrap());
}
fn complete(h: &mut epoch::History<Pending>) {
    let epoch::Phase::Pending {
        round: Some(round), ..
    } = &h.phase
    else {
        panic!("round")
    };
    until(|| round.poll());
    h.complete(now());
}
pub(crate) fn run() {
    let me = hart::hart_id().get();
    ARRIVED.fetch_add(1, AcqRel);
    until(|| ARRIVED.load(Acquire) == hart::hart_count());
    if me == 0 {
        let mut h = epoch::History::new();
        capture(&mut h, (true, true));
        PHASE.store(1, Release);
        if hart::hart_count() > 1 {
            until(|| HELD.load(Acquire) == 1);
            let epoch::Phase::Pending {
                round: Some(round), ..
            } = &h.phase
            else {
                panic!("round")
            };
            for _ in 0..64 {
                assert!(!round.poll());
                assert_eq!(h.snapshot().epoch, 0);
            }
        }
        PHASE.store(2, Release);
        complete(&mut h);
        let baseline = h.snapshot();
        assert_eq!((baseline.accessed, baseline.dirty), (0, 0));
        assert!(matches!(h.phase,epoch::Phase::Armed {synchronized,..} if synchronized>0));
        capture(&mut h, (true, true));
        PHASE.store(3, Release);
        if hart::hart_count() > 1 {
            until(|| HELD.load(Acquire) == 2);
            let epoch::Phase::Pending {
                round: Some(round), ..
            } = &h.phase
            else {
                panic!("round")
            };
            for _ in 0..64 {
                assert!(!round.poll());
                assert_eq!(h.snapshot(), baseline);
            }
        }
        PHASE.store(4, Release);
        complete(&mut h);
        let observed = h.snapshot();
        assert!(observed.accessed > baseline.epoch && observed.dirty > baseline.epoch);
        // Cancel can discard output; its original round still settles without
        // a PTE reference or renewed source authority.
        let cancelled = Pending::begin().unwrap();
        until(|| cancelled.poll());
        crate::putln!(
            "PASS activity-fences harts={} retained=1 stale-ack=1 cancel=1",
            hart::hart_count()
        );
        PHASE.store(5, Release);
    } else if me == 1 {
        until(|| PHASE.load(Acquire) >= 1 && MAILBOXES[me].pending().is_some());
        let old = MAILBOXES[me].pending().unwrap();
        HELD.store(1, Release);
        until(|| PHASE.load(Acquire) >= 2);
        service();
        until(|| PHASE.load(Acquire) >= 3 && MAILBOXES[me].pending().is_some_and(|r| r != old));
        // A delayed acknowledgement for the prior clear cannot complete this one.
        // SAFETY: this Hart already fenced and acknowledged old in phase two.
        unsafe { MAILBOXES[me].complete(old) };
        HELD.store(2, Release);
        until(|| PHASE.load(Acquire) >= 4);
        until(|| {
            service();
            PHASE.load(Acquire) == 5
        });
    } else {
        until(|| {
            service();
            PHASE.load(Acquire) == 5
        });
    }
}
