#[path = "../../src/basis/room/scheduler/slot/turn.rs"]
mod turn;
use turn::{Cause, Choice, Turn};
fn turn(now: u64, remaining: u32) -> Turn {
    Turn {
        stopped: false,
        ready: true,
        remaining,
        now,
        ceiling: 100,
    }
}
#[test]
fn a_ready_peer_receives_the_hart_at_either_budget_boundary() {
    assert_eq!(
        turn(99, 2).decide(Cause::Tick),
        Choice::Continue {
            spend: true,
            arm: true
        }
    );
    assert_eq!(turn(100, 2).decide(Cause::Tick), Choice::Rotate);
    assert_eq!(turn(99, 1).decide(Cause::Tick), Choice::Rotate);
    assert_eq!(turn(0, 8).decide(Cause::Yield), Choice::Rotate);
}
#[test]
fn stopping_wins_over_idle_and_yield_never_spends_a_tick() {
    for cause in [Cause::Tick, Cause::Yield] {
        for ready in [false, true] {
            assert_eq!(
                Turn {
                    stopped: true,
                    ready,
                    ..turn(100, 1)
                }
                .decide(cause),
                Choice::Park
            );
        }
    }
    assert_eq!(
        Turn {
            ready: false,
            ..turn(100, 1)
        }
        .decide(Cause::Tick),
        Choice::Continue {
            spend: false,
            arm: true
        }
    );
    assert_eq!(
        Turn {
            ready: false,
            ..turn(100, 8)
        }
        .decide(Cause::Yield),
        Choice::Continue {
            spend: false,
            arm: false
        }
    );
}
