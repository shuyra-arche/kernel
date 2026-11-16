//! Real concurrent CPU accesses through native mappings. The observer alias
//! stays retained while a writer's translation is retired, then Block/Vacate
//! permits reuse. No guest FENCE, FENCE.I or mock completion is used.
use crate::support::*;
use core::{
    fmt::Write,
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::*},
};
use kernel_api::{
    abi::memory::*,
    model::{cell::Placement, flat::*},
};
use kernel_client::{
    call::{room, unit},
    memory::Response,
};

static ADDRESS: AtomicUsize = AtomicUsize::new(0);
static STARTED: AtomicBool = AtomicBool::new(false);

extern "C" fn writer() -> ! {
    let pointer = ADDRESS.load(Acquire) as *const AtomicU64;
    for _ in 0..16 {
        unsafe { &*pointer }.fetch_add(1, Relaxed);
        room::sleep_until(room::clock() + 10_000_000).unwrap();
    }
    STARTED.store(true, Release);
    // The owner publishes the RW mapping before resuming this Task. Removal
    // deliberately faults this access; it cannot reach the observer alias.
    loop {
        unsafe { &*pointer }.fetch_add(1, Relaxed);
        // Also exercise leaving/re-entering the address space and migration.
        room::yield_();
    }
}

fn invoke(request: Request) -> Reply {
    let Call::Nest(call) = Call::from_request(request) else {
        panic!("Nest request")
    };
    let Response::Reply(reply) = kernel_client::memory::nest::invoke(call).unwrap() else {
        panic!("reply")
    };
    reply
}

pub fn run() {
    for release in [false, true] {
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
        let worker = spawn(writer);
        unit::resume(worker.into()).unwrap();
        let deadline = room::clock() + 5_000_000_000;
        while !STARTED.load(Acquire) {
            assert!(room::clock() < deadline);
            // Leave both ordinary schedulers available for placement; a busy
            // observer plus one idle Hart can keep stealing the writer back.
            room::sleep_until(room::clock() + 1_000_000).unwrap();
        }
        let action = if release {
            let Snapshot::Span(status) = inspect(Query::Span {
                token: memo.token(),
                nest: memo.nest(),
                span: memo.span(),
            }) else {
                panic!("reservation")
            };
            Action::DeleteSpan {
                token: memo.token(),
                nest: memo.nest(),
                span: memo.span(),
                expected: status.version,
            }
        } else {
            Action::Retract {
                memo,
                expected: memo_status(memo).generation,
            }
        };
        let request = request(action);
        let original = invoke(request);
        // Treat the original delivery as lost. Replaying the same request must
        // return the exact result even though the original generation is stale.
        assert_eq!(invoke(request), original);
        assert_eq!(
            invoke(crate::support::request(Action::InspectResult {
                domain: Domain::Nest,
                request: request.id().unwrap(),
            })),
            original
        );
        match original.outcome() {
            Outcome::Committed(_) => {}
            Outcome::Failed {
                failure: Failure::Busy,
                retained: Some(_),
            } if !release => {
                crate::recovery::retract(memo);
            }
            Outcome::Failed {
                failure: Failure::Busy,
                retained: Some(_),
            } => {
                let deadline = room::clock() + 5_000_000_000;
                loop {
                    let Snapshot::Span(status) = inspect(Query::Span {
                        token: memo.token(),
                        nest: memo.nest(),
                        span: memo.span(),
                    }) else {
                        panic!("reservation")
                    };
                    let reply = invoke(crate::support::request(Action::DeleteSpan {
                        token: memo.token(),
                        nest: memo.nest(),
                        span: memo.span(),
                        expected: status.version,
                    }));
                    match reply.outcome() {
                        Outcome::Committed(_) => break,
                        Outcome::Rejected(Failure::Busy | Failure::Stale)
                        | Outcome::Failed {
                            failure: Failure::Busy,
                            retained: Some(_),
                        } => {}
                        other => panic!("Release recovery: {other:?}"),
                    }
                    assert!(room::clock() < deadline);
                    room::yield_();
                }
            }
            other => panic!("retirement: {other:?}"),
        }
        // Explicit cleanup cannot rewrite the original failed request either.
        assert_eq!(invoke(request), original);
        let after = observed.load(Acquire);
        assert_ne!(after, 0);
        for _ in 0..16 {
            room::yield_();
            assert_eq!(
                observed.load(Acquire),
                after,
                "write after retirement completion"
            );
        }
        // The retired access terminates the writer through its page fault.
        join(worker);
        retract(observer);
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
        let (next, address) = reserve(space());
        insert(next, fresh);
        publish(next);
        assert_eq!(unsafe { (address.get() as *const u64).read_volatile() }, 0);
        retract(next);
        let _ = writeln!(
            Console,
            "PASS smp-writer task={} release={release}",
            worker.get()
        );
    }
    let _ = writeln!(
        Console,
        "PASS smp-retirement concurrent-access=1 retract=1 release=1 replay=1 block-vacate=1 reuse=1"
    );

    // Coherent data qualification must not silently qualify executable memory.
    let slot = create(1);
    mutate(Action::Occupy {
        slot,
        expected: slot_status(slot).version,
    });
    let (rx, _) = reserve_at(space(), Placement::lowest(4096).unwrap(), 10);
    insert(rx, slot);
    let rejected = invoke(request(Action::Publish {
        memo: rx,
        expected: memo_status(rx).generation,
        maintenance: None,
    }));
    assert_eq!(rejected.outcome(), Outcome::Rejected(Failure::Unsupported));
    let (rw, _) = reserve(space());
    insert(rw, slot);
    publish(rw);
    let rejected = invoke(request(Action::Protect {
        memo: rw,
        expected: memo_status(rw).generation,
        access: 10,
    }));
    assert_eq!(rejected.outcome(), Outcome::Rejected(Failure::Unsupported));
    retract(rw);
    let _ = writeln!(Console, "PASS smp-executable-rejected publish=1 protect=1");
}
