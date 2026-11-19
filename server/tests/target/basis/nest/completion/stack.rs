//! Runtime-owned test stacks. The initial User Task retains the actual Flat and
//! Span grants; Task stop/exit does not release either resource.
use crate::support::*;
use kernel_api::{
    abi::memory::*,
    address::*,
    model::{
        cell::Placement,
        flat::SlotPosition,
        nest::{MemoPosition, RangeVersion},
        unit::TaskStart,
    },
};
use kernel_client::{call::room, memory::Response};

pub struct Stack {
    pub start: TaskStart,
    pub base: VirtAddr,
    flat: SlotPosition,
    memo: MemoPosition,
    local: Option<MemoPosition>,
}
fn change(action: impl Fn() -> Action) -> Value {
    let deadline = room::clock() + 5_000_000_000;
    loop {
        let attempted = action();
        let Response::Reply(mut reply) = call(attempted) else {
            panic!("memory reply")
        };
        while matches!(reply.outcome(), Outcome::Pending(_)) {
            assert!(room::clock() < deadline);
            room::yield_();
            let Response::Reply(next) = call(Action::InspectResult {
                domain: reply.opcode().domain(),
                request: reply.request(),
            }) else {
                panic!("retained memory reply")
            };
            reply = next;
        }
        match reply.outcome() {
            Outcome::Committed(value) => return value,
            Outcome::Rejected(Failure::Busy | Failure::Stale) => {
                assert!(
                    room::clock() < deadline,
                    "stack retry expired: {attempted:?}, outcome={:?}",
                    reply.outcome(),
                );
                room::yield_();
            }
            other => panic!("memory mutation: {other:?}"),
        }
    }
}
impl Stack {
    pub fn new(entry: usize, args: &[usize], size: usize) -> Self {
        Self::in_space(space(), entry, args, size)
    }
    pub fn in_space(
        target_space: kernel_api::model::identity::PendId,
        entry: usize,
        args: &[usize],
        size: usize,
    ) -> Self {
        let size = size.max(16384).next_multiple_of(PAGE_SIZE);
        let pages = size / PAGE_SIZE;
        let flat = create(pages);
        let (memo, base) = reserve_pages(
            target_space,
            Placement::lowest(PAGE_SIZE).unwrap(),
            6,
            pages + 1,
        );
        let local = (target_space != space())
            .then(|| reserve_pages(space(), Placement::lowest(PAGE_SIZE).unwrap(), 6, pages));
        for page in 0..pages {
            let slot = SlotPosition::new(flat.token(), flat.flat(), page).unwrap();
            let target = MemoPosition::new(memo.range(), page + 1).unwrap();
            change(|| Action::Occupy {
                slot,
                expected: slot_status(slot).version,
            });
            change(|| Action::Insert {
                target,
                expected: memo_status(target).generation,
                source: Source::Flat {
                    position: slot,
                    expected: slot_status(slot).version,
                },
            });
            change(|| Action::Publish {
                memo: target,
                expected: memo_status(target).generation,
                maintenance: None,
            });
            if let Some((first, _)) = local {
                let target = MemoPosition::new(first.range(), page).unwrap();
                change(|| Action::Insert {
                    target,
                    expected: memo_status(target).generation,
                    source: Source::Flat {
                        position: slot,
                        expected: slot_status(slot).version,
                    },
                });
                change(|| Action::Publish {
                    memo: target,
                    expected: memo_status(target).generation,
                    maintenance: None,
                });
            }
        }
        let top = base.get() + PAGE_SIZE + size;
        let at = top - args.len() * 8;
        let local_at = local.map_or(at, |(_, base)| base.get() + size - args.len() * 8);
        for (index, word) in args.iter().enumerate() {
            unsafe {
                ((local_at + index * 8) as *mut usize).write(*word);
            }
        }
        Self {
            start: TaskStart {
                entry: VirtAddr::new(entry),
                stack_pointer: VirtAddr::new(at & !15),
                arguments: VirtAddr::new(if args.is_empty() { 0 } else { at }),
                argument_count: args.len(),
            },
            base,
            flat,
            memo,
            local: local.map(|(memo, _)| memo),
        }
    }
    /// Call only after the child is independently observed reclaimed. Release
    /// settles its real hardware obligations before the Flat may be deleted.
    pub fn retire(self) {
        for memo in [Some(self.memo), self.local].into_iter().flatten() {
            change(|| {
                let Snapshot::Span(status) = inspect(Query::Span {
                    range: memo.range(),
                }) else {
                    panic!("reservation")
                };
                Action::DeleteSpan {
                    range: memo.range(),
                    expected: RangeVersion::new(status.version.get()).unwrap(),
                }
            });
        }
        for memo in [Some(self.memo), self.local].into_iter().flatten() {
            kernel_client::call::pend::return_(memo.range()).unwrap();
        }
        change(|| {
            let Snapshot::Flat(status) = inspect(Query::Flat {
                token: self.flat.token(),
                flat: self.flat.flat(),
            }) else {
                panic!("flat")
            };
            Action::DeleteFlat {
                token: self.flat.token(),
                flat: self.flat.flat(),
                expected: status.version,
            }
        });
        kernel_client::call::pend::return_(self.flat.token()).unwrap();
    }
}

// Only the initial fixture Task owns this registry. Checks precede borrowing;
// no reference or borrow survives a Kernel call or scheduling boundary.
struct Owners(core::cell::UnsafeCell<[Option<(kernel_api::model::identity::TaskId, Stack)>; 64]>);
unsafe impl Sync for Owners {}
static OWNERS: Owners = Owners(core::cell::UnsafeCell::new([const { None }; 64]));
static OWNER: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
fn caller() -> usize {
    let kernel_api::model::unit::UnitTarget::Task(task) =
        task_info(kernel_api::model::unit::UnitQuery::Caller).target
    else {
        panic!("caller")
    };
    task.get()
}
pub fn init_owner() {
    OWNER.store(caller(), core::sync::atomic::Ordering::Release);
}
fn check_owner() {
    assert_eq!(caller(), OWNER.load(core::sync::atomic::Ordering::Acquire));
}
pub fn retain(task: kernel_api::model::identity::TaskId, stack: Stack) {
    check_owner();
    let owners = unsafe { &mut *OWNERS.0.get() };
    let slot = owners
        .iter_mut()
        .find(|slot| slot.is_none())
        .expect("bounded fixture stacks");
    *slot = Some((task, stack));
}
pub fn retire_joined(task: kernel_api::model::identity::TaskId) {
    check_owner();
    let stack = {
        let owners = unsafe { &mut *OWNERS.0.get() };
        owners
            .iter_mut()
            .find(|slot| slot.as_ref().is_some_and(|(id, _)| *id == task))
            .and_then(Option::take)
            .map(|(_, stack)| stack)
    };
    if let Some(stack) = stack {
        stack.retire();
    }
}
