#[path = "../../src/metal/riscv/console/loss.rs"]
mod loss;
#[path = "../../src/basis/unit/lifecycle/diagnostics/ring.rs"]
mod ring;
#[path = "../../src/entry/report/backtrace/walk.rs"]
mod walk;
use kernel_api::model::identity::TaskId;
use walk::{Backtrace, Source, Stop};

#[test]
fn compressed_return_pc_is_preserved_and_scan_is_not_a_proven_frame() {
    let trace = Backtrace::walk(
        |a| match a {
            0x1000 => Some(0x2082),
            0x1010 => Some(0),
            0x1018 => Some(0x2002),
            _ => Some(0),
        },
        0x1000,
        0x1020,
        0x1040,
        |pc| (0x2000..0x2100).contains(&pc),
    );
    assert_eq!(trace.stop, Stop::End);
    assert_eq!(trace.frames()[0].pc, 0x2002);
    assert_eq!(trace.frames()[0].sp, 0x1020);
    assert_eq!(trace.frames()[0].source, Source::FrameChain);
    assert_eq!(trace.frames()[1].source, Source::StackScan);
}
#[test]
fn corrupt_chains_stop_without_following_cycles_or_non_code() {
    for (caller, pc, stop) in [
        (0x1020, 0x2002, Stop::NonMonotonic),
        (0, 0x2001, Stop::InvalidPc),
        (0, 0x3000, Stop::InvalidPc),
    ] {
        let trace = Backtrace::walk(
            |a| match a {
                0x1010 => Some(caller),
                0x1018 => Some(pc),
                _ => None,
            },
            0x1000,
            0x1020,
            0x1100,
            |pc| (0x2000..0x2100).contains(&pc),
        );
        assert_eq!(trace.stop, stop); // 0x3000 represents ROOT/data, outside executable text.
    }
    let trace = Backtrace::walk(|_| None, 0x1000, 0x1020, 0x1100, |_| true);
    assert_eq!(trace.stop, Stop::Unreadable);
    let trace = Backtrace::walk(
        |_| panic!("out-of-bounds read"),
        usize::MAX - 7,
        0,
        usize::MAX,
        |_| true,
    );
    assert_eq!(trace.stop, Stop::End);
}
#[test]
fn hostile_memory_has_a_fixed_read_budget() {
    let mut reads = 0;
    let trace = Backtrace::walk(
        |_| {
            reads += 1;
            Some(0)
        },
        0x1000,
        usize::MAX,
        usize::MAX,
        |_| true,
    );
    assert_eq!(trace.stop, Stop::Bounds);
    assert!(reads <= walk::SPAN / 8 + 2 * walk::DEPTH);
}
#[test]
fn snapshot_survives_overwrite_and_utf8_truncation() {
    let mut ring = ring::Ring::new();
    for id in 0..10 {
        ring.push(ring::Entry::new(TaskId::new(id), 1, &"圜".repeat(100)));
    }
    let snapshot = ring.snapshot();
    ring.push(ring::Entry::new(TaskId::new(10), 2, "later"));
    assert_eq!(snapshot.overwritten, 2);
    assert_eq!(
        snapshot
            .entries()
            .iter()
            .map(|e| e.task.get())
            .collect::<Vec<_>>(),
        (2..10).collect::<Vec<_>>()
    );
    assert_eq!(snapshot.entries()[0].reason, 1);
    assert!(snapshot.entries()[0].note().ends_with('圜'));
    assert_eq!(ring.snapshot().overwritten, 3);
}
#[test]
fn console_loss_counts_zero_partial_errors_and_bad_counts() {
    let loss = loss::Loss::new();
    for n in [Some(0), Some(2), None, Some(6)] {
        loss.record(5, n);
    }
    assert_eq!(loss.take(), 18);
    assert_eq!(loss.take(), 0);
    loss.record(5, Some(5));
    assert_eq!(loss.take(), 0);
}
