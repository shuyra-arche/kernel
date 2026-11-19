//! Real CPU fault and finite read continuations, using only public grants/calls.
use super::support::*;
use core::{
    fmt::Write,
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
};
use kernel_api::{
    abi::memory::*,
    address::VirtAddr,
    model::{nest::*, unit::*, wait::Wait},
};
use kernel_client::call::{room, sign, unit};
static EXPECTED: AtomicU64 = AtomicU64::new(0);
static ADDRESS: AtomicUsize = AtomicUsize::new(0);
extern "C" fn reader() -> ! {
    let value = unsafe { (ADDRESS.load(Ordering::Acquire) as *const u64).read_volatile() };
    let expected = EXPECTED.load(Ordering::Acquire);
    assert_eq!(value, expected);
    if expected != 0 {
        for i in 0..512 {
            assert_eq!(
                unsafe {
                    ((ADDRESS.load(Ordering::Acquire) + i * 8) as *const u64).read_volatile()
                },
                expected ^ (i as u64)
            );
        }
    }
    unit::exit(42, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
pub fn configure(memo: MemoPosition, route: Option<FaultRoute>) -> FaultEpoch {
    let Snapshot::Reservation(status) = inspect(Query::Reservation {
        token: memo.token(),
        nest: memo.nest(),
        reservation: memo.reservation(),
    }) else {
        panic!("Reservation")
    };
    let Snapshot::FaultRoute(fault) = inspect(Query::FaultRoute {
        token: memo.token(),
        nest: memo.nest(),
        reservation: memo.reservation(),
    }) else {
        panic!("FaultRoute")
    };
    let Value::FaultRoute { epoch, .. } = mutate(Action::FaultRoute {
        token: memo.token(),
        nest: memo.nest(),
        reservation: memo.reservation(),
        expected: status.version,
        epoch: fault.epoch,
        route,
    }) else {
        panic!("route")
    };
    epoch
}
pub fn event(memo: MemoPosition, epoch: FaultEpoch, wait: Wait) -> FaultEvent {
    let Snapshot::FaultEvent(event) = inspect(Query::FaultEvent {
        token: memo.token(),
        nest: memo.nest(),
        reservation: memo.reservation(),
        epoch,
        after: 0,
        wait,
    }) else {
        panic!("event")
    };
    event
}
#[derive(Clone, Copy)]
enum Scenario {
    Recover,
    RouteLoss,
    Timeout,
}
pub fn run() {
    for mode in [Scenario::Recover, Scenario::RouteLoss, Scenario::Timeout] {
        let slot = create(1);
        let (memo, address) = reserve(space());
        insert(memo, slot);
        let signal = sign::create(license()).unwrap();
        let epoch = configure(
            memo,
            Some(FaultRoute {
                sign: signal,
                timeout_ns: if matches!(mode, Scenario::Timeout) {
                    100_000_000
                } else {
                    2_000_000_000
                },
            }),
        );
        ADDRESS.store(address.get(), Ordering::Release);
        let task = spawn(reader);
        unit::resume(UnitTarget::Task(task)).unwrap();
        let first = event(memo, epoch, Wait::At(room::clock() + 1_000_000_000));
        assert_eq!(first.flat, slot.flat());
        assert_eq!(first.generation, memo_status(memo).generation);
        assert_eq!(
            event(memo, epoch, Wait::Poll),
            first,
            "event reads must not consume"
        );
        match mode {
            Scenario::Recover => {
                mutate(Action::Occupy {
                    slot,
                    expected: slot_status(slot).version,
                });
                // A supplied Page alone cannot complete the original CPU fault.
                assert_ne!(
                    task_info(UnitQuery::Target(UnitTarget::Task(task))).state,
                    UnitState::Exited
                );
                publish(memo);
                assert_eq!(join(task).result, Some(42));
                retract(memo);
            }
            Scenario::RouteLoss => {
                configure(memo, None);
                assert_eq!(
                    join(task).exit,
                    Some((ExitCause::Fault, Failure::HandlerGone as i64 as u64))
                );
            }
            Scenario::Timeout => {
                assert_eq!(
                    join(task).exit,
                    Some((ExitCause::Fault, Failure::TimedOut as i64 as u64))
                );
            }
        }
        sign::delete(signal).unwrap();
    }
    let _ = writeln!(
        Console,
        "PASS native-fault recovery=1 nonconsuming=1 publication-required=1 route-loss=1 timeout=1"
    );
}

pub fn start_reader(
    memo: MemoPosition,
    address: VirtAddr,
    expected: u64,
) -> (kernel_api::model::identity::TaskId, FaultEpoch) {
    EXPECTED.store(expected, Ordering::Release);
    ADDRESS.store(address.get(), Ordering::Release);
    let signal = sign::create(license()).unwrap();
    let epoch = configure(
        memo,
        Some(FaultRoute {
            sign: signal,
            timeout_ns: 5_000_000_000,
        }),
    );
    let task = spawn(reader);
    unit::resume(UnitTarget::Task(task)).unwrap();
    (task, epoch)
}
