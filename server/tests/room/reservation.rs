#[path = "../../src/basis/unit/task/schedule/account.rs"]
mod account;
use account::{Account, Contract, analyze, priorities};
use kernel_api::model::unit::{Schedule, ScheduleInfo};

fn spec() -> Schedule {
    Schedule {
        hart: 1,
        priority: 4,
        ceiling: 8,
        budget: 1_000_000,
        period: 100_000_000,
        latency: 1_000_000,
        extensions: 0,
    }
}
fn contract(priority: u64, ceiling: u64, budget: u64, period: u64) -> Contract {
    Contract {
        spec: Schedule {
            priority,
            ceiling,
            ..spec()
        },
        budget,
        period,
        latency: 1,
        response: 0,
    }
}

#[test]
fn yield_block_and_delayed_wakeup_preserve_one_window_of_credit() {
    let mut account = Account::new(20, 100);
    account.enter(10);
    account.leave(17);
    assert_eq!(account.remaining(70), 13);
    account.enter(70);
    account.leave(80);
    assert_eq!(account.remaining(109), 3);
    assert_eq!(account.remaining(110), 20);
    assert_eq!(account.remaining(1010), 20);
    account.enter(1010);
    account.leave(1030);
    assert_eq!(account.remaining(1050), 0);
    assert_eq!(account.deadline(1050), 1110);
    assert_eq!(account.consumed, 37);
}

#[test]
fn late_interrupts_are_charged_across_windows_in_constant_work() {
    let mut account = Account::new(20, 100);
    account.enter(0);
    account.charge(235);
    assert_eq!(account.consumed, 235);
    assert_eq!(account.overruns, 175);
    assert_eq!(account.remaining(235), 0);
    assert_eq!(account.deadline(235), 300);
    account.leave(239);
    assert_eq!(account.consumed, 239);
    assert_eq!(account.overruns, 179);
    assert_eq!(account.remaining(300), 20);
}

#[test]
fn every_execution_segment_is_counted_once() {
    let mut account = Account::new(37, 100);
    let mut consumed = 0;
    let mut now = 0;
    for i in 0..200 {
        now += 101;
        assert_eq!(account.remaining(now), 37);
        account.enter(now);
        for _ in 0..i % 17 {
            now += 1;
            account.charge(now);
            consumed += 1;
        }
        account.leave(now);
        assert_eq!(account.consumed, consumed);
    }
}

#[test]
fn admission_checks_all_tasks_at_possible_inherited_priorities() {
    let mut tasks = [
        contract(9, 9, 2, 100),
        contract(5, 5, 3, 100),
        contract(1, 9, 2, 100),
    ];
    assert!(analyze(&mut tasks));
    assert_eq!(tasks[0].response, 7); // low base server can inherit 9
    assert_eq!(tasks[1].response, 12);
    assert_eq!(tasks[2].response, 13);
    let mut overloaded = [contract(1, 9, 40, 100), contract(9, 9, 40, 100)];
    assert!(!analyze(&mut overloaded));
    let mut overflow = [Contract {
        budget: u64::MAX - 1,
        period: u64::MAX,
        latency: 2,
        ..contract(1, 1, 1, 2)
    }];
    assert!(!analyze(&mut overflow));
}

#[test]
fn transitive_inheritance_restores_other_donors_after_cancellation() {
    let base = [9, 2, 1, 7];
    let ceiling = [9, 9, 9, 7];
    let mut effective = [0; 4];
    assert!(priorities(
        &base,
        &ceiling,
        &[Some(1), Some(2), None, Some(2)],
        &mut effective
    ));
    assert_eq!(effective, [9, 9, 9, 7]);
    assert!(priorities(
        &base,
        &ceiling,
        &[None, Some(2), None, Some(2)],
        &mut effective
    ));
    assert_eq!(effective, [9, 2, 7, 7]);
    assert!(priorities(
        &base,
        &ceiling,
        &[None, None, None, None],
        &mut effective
    ));
    assert_eq!(effective, base);
}

#[test]
fn dependency_admission_rejects_cycles_depth_and_ceiling_escalation() {
    assert!(!priorities(&[9, 2], &[9, 8], &[Some(1), None], &mut [0; 2]));
    assert!(!priorities(
        &[9, 2],
        &[9, 9],
        &[Some(1), Some(0)],
        &mut [0; 2]
    ));
    let mut edges = [None; 10];
    for (at, edge) in edges.iter_mut().enumerate().take(9) {
        *edge = Some(at + 1);
    }
    assert!(!priorities(&[1; 10], &[9; 10], &edges, &mut [0; 10]));
    edges[8] = None;
    assert!(priorities(&[1; 10], &[9; 10], &edges, &mut [0; 10]));
}

#[test]
fn schedule_wire_values_reject_unknown_extensions_and_invalid_ranges() {
    let valid = spec();
    assert_eq!(Schedule::from_bytes(&valid.to_bytes()), Some(valid));
    for value in [
        Schedule { hart: 0, ..valid },
        Schedule {
            priority: 0,
            ..valid
        },
        Schedule {
            priority: 9,
            ..valid
        },
        Schedule {
            ceiling: 32,
            ..valid
        },
        Schedule {
            budget: valid.period,
            ..valid
        },
        Schedule {
            extensions: 4,
            ..valid
        },
    ] {
        assert!(Schedule::from_bytes(&value.to_bytes()).is_none());
    }
    let info = ScheduleInfo {
        schedule: valid,
        effective: 8,
        remaining: 2,
        replenishment: 3,
        consumed: 4,
        overruns: 5,
        response: 6,
    };
    assert_eq!(ScheduleInfo::from_bytes(&info.to_bytes()), Some(info));
}
