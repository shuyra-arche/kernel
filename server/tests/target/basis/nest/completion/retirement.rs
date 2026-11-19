//! A real CPU writer and a retained alias: completing a withdrawal must
//! prevent stale writes before the old slot can be vacated and reused.
//! Single-Hart QEMU is a baseline; four-Hart execution is explicit only.
use crate::support::*;
use core::{
    fmt::Write,
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::*},
};
use kernel_api::{
    abi::memory::*,
    model::{cell::Placement, flat::*, nest::{MemoPosition, RangeVersion}},
};
use kernel_client::{
    call::{room, unit},
    memory::Response,
};

static ADDRESS: AtomicUsize = AtomicUsize::new(0);
static STARTED: AtomicBool = AtomicBool::new(false);

extern "C" fn writer() -> ! {
    let address = ADDRESS.load(Acquire) as *const AtomicU64;
    for _ in 0..16 {
        unsafe { &*address }.fetch_add(1, Relaxed);
        room::sleep_until(room::clock() + 10_000_000).unwrap();
    }
    STARTED.store(true, Release);
    loop {
        // Once the owner withdraws the mapping, this must fault instead of
        // modifying the still-live observer alias.
        unsafe { &*address }.fetch_add(1, Relaxed);
        room::yield_();
    }
}

fn invoke(request: Request) -> Reply {
    let Call::Nest(call) = Call::from_request(request) else {
        panic!("expected Nest call")
    };
    let Response::Reply(reply) = kernel_client::memory::nest::invoke(call).unwrap() else {
        panic!("expected a Nest reply")
    };
    reply
}

fn complete(action: Action) -> Outcome {
    let request = request(action);
    let deadline = room::clock() + 8_000_000_000;
    let mut reply = invoke(request);
    while matches!(reply.outcome(), Outcome::Pending(_)) {
        assert!(room::clock() < deadline, "Nest result remained pending");
        room::yield_();
        reply = invoke(crate::support::request(Action::InspectResult {
            domain: Domain::Nest,
            request: request.id().unwrap(),
        }));
    }
    // The journal's final result must remain identical across replay,
    // even when the original view or Memo version is now stale.
    assert_eq!(invoke(request), reply);
    reply.outcome()
}

fn span_status(memo: MemoPosition) -> kernel_api::model::nest::SpanStatus {
    let Snapshot::Span(status) = inspect(Query::Span {
        range: memo.range(),
    }) else {
        panic!("Span observation")
    };
    status
}

fn retire(memo: MemoPosition, delete: bool) {
    let deadline = room::clock() + 8_000_000_000;
    loop {
        let action = if delete {
            Action::DeleteSpan {
                range: memo.range(),
                expected: RangeVersion::new(span_status(memo).version.get()).unwrap(),
            }
        } else {
            Action::Retract {
                memo,
                expected: memo_status(memo).generation,
            }
        };
        match complete(action) {
            Outcome::Committed(_) => return,
            Outcome::Failed {
                failure: Failure::Busy,
                retained: Some(_),
            } | Outcome::Rejected(Failure::Busy | Failure::Stale) => {
                assert!(room::clock() < deadline, "retirement did not settle");
                room::yield_();
            }
            other => panic!("retirement result: {other:?}"),
        }
    }
}

pub fn run() {
    for delete in [false, true] {
        let slot = create(1);
        mutate(Action::Occupy {
            slot,
            expected: slot_status(slot).version,
        });
        let (memo, address) = reserve(space());
        let (observer, observed_address) = reserve(space());
        insert(memo, slot);
        insert(observer, slot);
        publish(memo);
        publish(observer);

        let observed = unsafe { &*(observed_address.get() as *const AtomicU64) };
        observed.store(0, Relaxed);
        STARTED.store(false, Relaxed);
        ADDRESS.store(address.get(), Release);
        let task = spawn(writer);
        unit::resume(task.into()).unwrap();

        let deadline = room::clock() + 8_000_000_000;
        while !STARTED.load(Acquire) {
            assert!(room::clock() < deadline, "writer never became resident");
            room::sleep_until(room::clock() + 1_000_000).unwrap();
        }

        retire(memo, delete);
        let stopped_at = observed.load(Acquire);
        assert_ne!(stopped_at, 0);
        for _ in 0..16 {
            room::yield_();
            assert_eq!(
                observed.load(Acquire), stopped_at,
                "writer modified retained alias after retirement"
            );
        }
        let stopped = join(task);
        assert!(stopped.exit.is_some(), "retired writer did not terminate");
        if !delete {
            retire(memo, true);
        }
        retire(observer, false);
        assert_eq!(slot_status(slot).obligations, 0);
        let maintenance = block(slot);
        mutate(Action::Vacate {
            slot,
            expected: slot_status(slot).version,
            maintenance,
        });
        assert_eq!(slot_status(slot).phase, SlotPhase::Missing);
        mutate(Action::Allow { maintenance });

        let fresh = create(1);
        mutate(Action::Occupy {
            slot: fresh,
            expected: slot_status(fresh).version,
        });
        let (next, at) = reserve_at(space(), Placement::lowest(4096).unwrap(), 6);
        insert(next, fresh);
        publish(next);
        assert_eq!(unsafe { (at.get() as *const u64).read_volatile() }, 0);
        retire(next, false);
        let _ = writeln!(
            Console,
            "PASS nest-writer task={} delete={delete}",
            task.get()
        );
    }
    let _ = writeln!(
        Console,
        "PASS nest-completion abi=14 alias=retained withdrawal=2 replay=2 task-fault=2 reuse=2"
    );
}
