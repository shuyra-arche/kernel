//! Runtime-owned test stacks. The initial User Task retains the actual Flat and
//! Reservation grants; Task stop/exit does not release either resource.
use crate::support::*;
use kernel_api::{
    abi::memory::*,
    address::*,
    model::{cell::Placement, flat::SlotPosition, nest::MemoPosition, unit::TaskStart},
};
use kernel_client::{call::room, memory::Response};

pub struct Stack {
    pub start: TaskStart,
    pub base: VirtAddr,
    flat: SlotPosition,
    memo: MemoPosition,
    guard: Option<MemoPosition>,
}
fn change(action: impl Fn() -> Action) -> Value {
    let deadline = room::clock() + 5_000_000_000;
    loop {
        let Response::Reply(mut reply) = call(action()) else {
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
                assert!(room::clock() < deadline);
                room::yield_();
            }
            other => panic!("memory mutation: {other:?}"),
        }
    }
}
impl Stack {
    pub fn new(entry: usize, args: &[usize], size: usize) -> Self {
        Self::prepare(entry, args, size, false)
    }
    /// Optional RT admission prepares a fully resident body Reservation, with
    /// its absent guard retained separately by this Runtime owner.
    pub fn resident(entry: usize, args: &[usize], size: usize) -> Self {
        Self::prepare(entry, args, size, true)
    }
    fn prepare(entry: usize, args: &[usize], size: usize, resident: bool) -> Self {
        let size = size.max(16384).next_multiple_of(PAGE_SIZE);
        let pages = size / PAGE_SIZE;
        let flat = create(pages);
        let (memo, base, guard, first) = if resident {
            let (memo, body) =
                reserve_pages(space(), Placement::highest(PAGE_SIZE).unwrap(), 6, pages);
            let (guard, base) = reserve_pages(
                space(),
                Placement::exact(body - PAGE_SIZE, PAGE_SIZE).unwrap(),
                6,
                1,
            );
            (memo, base, Some(guard), 0)
        } else {
            let (memo, base) =
                reserve_pages(space(), Placement::lowest(PAGE_SIZE).unwrap(), 6, pages + 1);
            (memo, base, None, 1)
        };
        for page in 0..pages {
            let slot = SlotPosition::new(flat.token(), flat.flat(), page).unwrap();
            let target =
                MemoPosition::new(memo.token(), memo.nest(), memo.reservation(), page + first)
                    .unwrap();
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
        }
        let top = base.get() + PAGE_SIZE + size;
        let at = top - args.len() * 8;
        for (index, word) in args.iter().enumerate() {
            unsafe {
                ((at + index * 8) as *mut usize).write(*word);
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
            guard,
        }
    }
    /// Call after the child is independently observed reclaimed, or while it is
    /// still dormant and has never run. Release
    /// settles its real hardware obligations before the Flat may be deleted.
    pub fn retire(self) {
        for memo in [Some(self.memo), self.guard].into_iter().flatten() {
            change(|| {
                let Snapshot::Reservation(status) = inspect(Query::Reservation {
                    token: memo.token(),
                    nest: memo.nest(),
                    reservation: memo.reservation(),
                }) else {
                    panic!("reservation")
                };
                Action::Release {
                    token: memo.token(),
                    nest: memo.nest(),
                    reservation: memo.reservation(),
                    expected: status.version,
                }
            });
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
    }
}
