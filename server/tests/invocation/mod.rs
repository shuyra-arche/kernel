#![feature(allocator_ext)]
extern crate alloc;
// Production descriptor-copy, dispatch and result transaction. Resource/Task
// scheduling and user buffers are fixtures; this is not a trap/MMU guest test.
mod entry;
#[path = "../../src/basis/unit/task/memory_result.rs"]
pub(crate) mod memory_result;
mod support;
use kernel_api::{
    abi::memory::{
        Action, Call, Delivery, Domain, Failure, Outcome, Reply, Request, RequestId, Value,
    },
    model::{
        flat::{FlatId, SlotPosition, SlotVersion},
        identity::PendId,
        nest::{MemoGeneration, MemoPosition, NestId, ReservationId},
    },
};
use memory_result::Transition;
use std::sync::atomic::Ordering;
use support::*;
pub(crate) mod basis {
    pub(crate) mod room {
        pub(crate) enum Handoff<T> {
            Resume(T),
            #[expect(
                dead_code,
                reason = "this descriptor fixture rejects fault waits; real switches run in the native guest"
            )]
            Switch(usize),
        }
    }
    pub(crate) mod nest {
        pub(crate) mod fault {
            pub(crate) mod read {
                pub(crate) struct Read;
                impl Read {
                    pub(crate) fn new(
                        _: &std::sync::Arc<crate::support::Task>,
                        _: kernel_api::abi::memory::Query,
                        _: kernel_api::address::VirtAddr,
                    ) -> Result<Self, kernel_api::abi::memory::Failure> {
                        Err(kernel_api::abi::memory::Failure::Unsupported)
                    }
                }
                pub(crate) fn park(_: Read) -> crate::basis::room::Handoff<()> {
                    panic!("scheduler outside invocation fixture")
                }
            }
        }
    }
    pub(crate) use crate::support::commit;
    pub(crate) mod unit {
        pub(crate) mod task {
            pub(crate) use crate::memory_result;
            pub(crate) use crate::support::{Task, TaskIdent};
        }
    }
    pub(crate) mod cell {
        pub(crate) mod change {
            pub(crate) fn maintain() {
                assert_eq!(crate::support::commit_depth(), 0);
            }
        }
    }
}
pub(crate) mod metal {
    pub(crate) mod switcher {
        pub(crate) mod context {
            pub(crate) use crate::support::{Gprs, TrapContext};
        }
    }
}

fn request(id: u64, publish: bool) -> Request {
    Request::new(
        Some(RequestId::new(id).unwrap()),
        if publish {
            Action::Publish {
                memo: MemoPosition::new(
                    PendId::mint(3),
                    NestId::new(4).unwrap(),
                    ReservationId::from_bytes(&5u64.to_le_bytes()).unwrap(),
                    0,
                )
                .unwrap(),
                expected: MemoGeneration::INITIAL,
                maintenance: None,
            }
        } else {
            Action::Occupy {
                slot: SlotPosition::new(PendId::mint(3), FlatId::new(4).unwrap(), 0).unwrap(),
                expected: SlotVersion::INITIAL,
            }
        },
    )
    .unwrap()
}
fn invoke(task: &Task, request: Request, output: usize) -> (Delivery, Option<Reply>) {
    task.ident.team.nest.bytes.lock().unwrap()[..Request::WIDTH]
        .copy_from_slice(&request.to_bytes());
    let mut frame = TrapContext::default();
    entry::call::invoke(
        &mut frame,
        &task.ident,
        Call::from_request(request).number(),
        [0x1000, output, 0, 0, 0, 0],
    );
    let delivery = Delivery::from_registers(frame.gpr.0.map(|n| n as u64)).unwrap();
    let reply = if delivery == Delivery::Written {
        Reply::from_bytes(
            &task.ident.team.nest.bytes.lock().unwrap()
                [output - 0x1000..output - 0x1000 + Reply::WIDTH],
        )
    } else {
        None
    };
    (delivery, reply)
}

#[test]
fn nest_creation_prepares_outside_commit_and_lost_output_replays_one_result() {
    let task = Task::new();
    let req = Request::new(
        Some(RequestId::new(1).unwrap()),
        Action::CreateNest {
            license: PendId::mint(3),
        },
    )
    .unwrap();
    task.ident.team.nest.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        invoke(&task, req, 0x1100).0,
        Delivery::Failed(Failure::ReplyFault)
    );
    assert_eq!(task.effects.load(Ordering::SeqCst), 1);
    task.ident.team.nest.fail.store(false, Ordering::SeqCst);
    let first = invoke(&task, req, 0x1100).1.unwrap();
    assert!(matches!(
        first.outcome(),
        Outcome::Committed(Value::CreatedNest { .. })
    ));
    for _ in 0..8 {
        assert_eq!(invoke(&task, req, 0x1100).1, Some(first));
    }
    assert_eq!(task.effects.load(Ordering::SeqCst), 1);
}

#[test]
fn deferred_completion_preserves_pending_replay_and_releases_journal_capacity() {
    let task = Task::new();
    task.mode.store(3, Ordering::SeqCst);
    for id in 1..=(kernel_api::abi::memory::RESULT_CAPACITY * 2) as u64 {
        let req = request(id, true);
        let first = invoke(&task, req, 0x1100).1.unwrap();
        assert!(matches!(first.outcome(), Outcome::Pending(_)));
        assert_eq!(invoke(&task, req, 0x1100).1, Some(first));
        assert_eq!(task.effects.load(Ordering::SeqCst), id as usize);
        task.completion
            .lock()
            .take()
            .unwrap()
            .finish(Transition::Published);
        let done = invoke(&task, req, 0x1100).1.unwrap();
        assert!(matches!(done.outcome(), Outcome::Committed(_)));
        assert_eq!(task.effects.load(Ordering::SeqCst), id as usize);
    }
}

#[test]
fn deferred_cancel_and_hardware_failure_are_terminal_without_reexecution() {
    for hardware in [false, true] {
        let task = Task::new();
        task.mode.store(3, Ordering::SeqCst);
        let req = request(1, true);
        invoke(&task, req, 0x1100);
        let completion = task.completion.lock().take().unwrap();
        if hardware {
            completion.finish(Transition::Failed(Failure::Hardware));
        } else {
            completion.finish(Transition::Retired);
        }
        let result = invoke(&task, req, 0x1100).1.unwrap();
        assert!(matches!(
            result.outcome(),
            Outcome::Failed {
                retained: Some(_),
                ..
            }
        ));
        assert_eq!(task.effects.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn deferred_result_does_not_keep_dead_task_alive() {
    let task = Task::new();
    task.mode.store(3, Ordering::SeqCst);
    invoke(&task, request(1, true), 0x1100);
    let weak = std::sync::Arc::downgrade(&task);
    let completion = task.completion.lock().take().unwrap();
    drop(task);
    assert!(weak.upgrade().is_none());
    completion.finish(Transition::Published);
}

#[test]
fn lost_output_and_overlapping_buffers_do_not_execute_the_same_mutation_twice() {
    let task = Task::new();
    task.ident.team.nest.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        invoke(&task, request(1, false), 0x1100).0,
        Delivery::Failed(Failure::ReplyFault)
    );
    assert_eq!(task.effects.load(Ordering::SeqCst), 1);
    task.ident.team.nest.fail.store(false, Ordering::SeqCst);
    for _ in 0..8 {
        let (delivery, reply) = invoke(&task, request(1, false), 0x1000);
        assert_eq!(delivery, Delivery::Written);
        assert!(matches!(
            reply.unwrap().outcome(),
            Outcome::Committed(Value::Slot { .. })
        ));
    }
    assert_eq!(task.effects.load(Ordering::SeqCst), 1);
}

#[test]
fn accepted_hardware_failure_keeps_result_and_replay_never_republishes() {
    let task = Task::new();
    task.mode.store(2, Ordering::SeqCst);
    let reply = invoke(&task, request(1, true), 0x1100).1.unwrap();
    assert!(matches!(
        reply.outcome(),
        Outcome::Failed {
            failure: Failure::Busy,
            retained: Some(Value::Memo { .. })
        }
    ));
    task.mode.store(0, Ordering::SeqCst);
    assert_eq!(invoke(&task, request(1, true), 0x1100).1, Some(reply));
    let query = Request::new(
        None,
        Action::InspectResult {
            domain: Domain::Nest,
            request: RequestId::new(1).unwrap(),
        },
    )
    .unwrap();
    assert_eq!(invoke(&task, query, 0x1100).1, Some(reply));
    assert_eq!(task.effects.load(Ordering::SeqCst), 1);
}

#[test]
fn rejection_and_pending_attempt_are_distinct_and_fences_hold_no_ledger_or_result_lock() {
    let task = Task::new();
    task.mode.store(1, Ordering::SeqCst);
    assert!(matches!(
        invoke(&task, request(1, true), 0x1100).1.unwrap().outcome(),
        Outcome::Rejected(Failure::Denied)
    ));
    assert_eq!(task.effects.load(Ordering::SeqCst), 0);
    task.mode.store(0, Ordering::SeqCst);
    assert!(matches!(
        invoke(&task, request(2, true), 0x1100).1.unwrap().outcome(),
        Outcome::Committed(Value::Memo { .. })
    ));
    assert_eq!(task.effects.load(Ordering::SeqCst), 1);
}

#[test]
fn descriptor_opcode_mismatch_rejects_before_admission_or_effects() {
    let task = Task::new();
    task.ident.team.nest.bytes.lock().unwrap()[..Request::WIDTH]
        .copy_from_slice(&request(1, true).to_bytes());
    let mut frame = TrapContext::default();
    entry::call::invoke(
        &mut frame,
        &task.ident,
        Call::from_request(request(1, false)).number(),
        [0x1000, 0x1100, 0, 0, 0, 0],
    );
    assert_eq!(
        Delivery::from_registers(frame.gpr.0.map(|n| n as u64)),
        Some(Delivery::Failed(Failure::Invalid))
    );
    assert!(task.memory_results.lock().is_none());
    assert_eq!(task.effects.load(Ordering::SeqCst), 0);
}
