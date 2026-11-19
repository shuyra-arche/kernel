use core::{
    fmt::Write,
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
};
use kernel_api::{
    abi::memory::*,
    address::*,
    model::{cell::*, flat::*, identity::*, nest::*, pend::*},
};
use kernel_client::{
    call::{debug, pend, room},
    memory::Response,
};
pub struct Console;
impl core::fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for bytes in s.as_bytes().chunks(128) {
            let mut at = 0;
            while at < bytes.len() {
                let written = debug::put(
                    VirtAddr::new(bytes[at..].as_ptr() as usize),
                    bytes.len() - at,
                )
                .map_err(|_| core::fmt::Error)?;
                if written == 0 {
                    return Err(core::fmt::Error);
                }
                at += written;
            }
        }
        Ok(())
    }
}
static IMAGE: AtomicUsize = AtomicUsize::new(0);
static IMAGE_LEN: AtomicUsize = AtomicUsize::new(0);
static NEXT: AtomicU64 = AtomicU64::new(1);
pub fn request(action: Action) -> Request {
    let id = if matches!(action, Action::Inspect(_) | Action::InspectResult { .. }) {
        None
    } else {
        RequestId::new(NEXT.fetch_add(1, Ordering::Relaxed))
    };
    Request::new(id, action).expect("canonical request")
}
pub fn call(action: Action) -> Response {
    crate::recovery::block(action)
    .unwrap_or_else(|e| panic!("{action:?}: {e:?}"))
}
#[track_caller]
pub fn mutate(action: Action) -> Value {
    let deadline = room::clock() + 5_000_000_000;
    let r = loop {
        let Response::Reply(r) = call(action) else { panic!("mutation reply") };
        if r.outcome() != Outcome::Rejected(Failure::Busy) { break r; }
        assert!(room::clock() < deadline, "rejected Busy did not progress");
        room::yield_();
    };
    let Outcome::Committed(v) = r.outcome() else {
        panic!("{action:?}: {:?}", r.outcome())
    };
    v
}
pub fn inspect(query: Query) -> Snapshot {
    let Response::Observation(v) = call(Action::Inspect(query)) else {
        panic!("observation")
    };
    v.snapshot()
}
pub fn pend_info(token: PendId) -> PendInfo {
    let mut w = [0; PendInfo::WORDS];
    pend::inspect(token, VirtAddr::new(w.as_mut_ptr() as usize)).unwrap();
    PendInfo::from_words(w).unwrap()
}
pub fn find(test: impl Fn(PendInfo) -> bool) -> PendInfo {
    let mut after = PendId::NONE;
    loop {
        let mut w = [0; PendInfo::WORDS];
        assert_eq!(
            pend::collect(after, VirtAddr::new(w.as_mut_ptr() as usize), 1).unwrap(),
            1
        );
        let i = PendInfo::from_words(w).unwrap();
        if test(i) {
            return i;
        }
        after = i.token;
    }
}
pub fn license() -> PendId {
    find(|p| p.target == Target::None).token
}
pub fn space() -> PendId {
    find(|p| {
        if p.limits.scope != PendScope::Nest(NestScope::Whole) {
            return false;
        }
        let Target::Nest(nest) = p.target else {
            return false;
        };
        let Snapshot::Nest(s) = inspect(Query::Nest {
            token: p.token,
            nest,
        }) else {
            return false;
        };
        s.info.kind == NestKind::Internal
    })
    .token
}
pub fn boot(at: usize, count: usize) {
    use kernel_api::boot::{args, ledger::Entries};
    assert_eq!(count, args::LEN);
    assert_eq!(kernel_api::abi::VERSION, 13);
    let a = unsafe { core::slice::from_raw_parts(at as *const usize, count) };
    IMAGE.store(a[args::VIEW], Ordering::Relaxed);
    IMAGE_LEN.store(a[args::VIEW_LEN], Ordering::Relaxed);
    let bytes =
        unsafe { core::slice::from_raw_parts(a[args::LEDGER] as *const u8, a[args::LEDGER_LEN]) };
    assert!(Entries::new(bytes).is_some());
    let _ = writeln!(
        Console,
        "START native-memory abi={} ledger={}",
        kernel_api::abi::VERSION,
        kernel_api::boot::ledger::VERSION
    );
}
pub fn create(count: usize) -> SlotPosition {
    let Value::CreatedFlat { token, flat, .. } = mutate(Action::CreateFlat {
        license: license(),
        count,
        level: CellLevel::new(0).unwrap(),
    }) else {
        panic!("Flat")
    };
    SlotPosition::new(token, flat, 0).unwrap()
}
pub fn slot_status(slot: SlotPosition) -> SlotStatus {
    let Snapshot::Slot(s) = inspect(Query::Slot(slot)) else {
        panic!("Slot")
    };
    s
}
pub fn memo_status(memo: MemoPosition) -> MemoStatus {
    let Snapshot::Memo(s) = inspect(Query::Memo(memo)) else {
        panic!("Memo")
    };
    s
}
pub fn reserve(token: PendId) -> (MemoPosition, VirtAddr) {
    reserve_at(token, Placement::lowest(4096).unwrap(), 6)
}
pub fn reserve_at(token: PendId, placement: Placement, access: u64) -> (MemoPosition, VirtAddr) {
    reserve_pages(token, placement, access, 1)
}
pub fn reserve_pages(
    token: PendId,
    placement: Placement,
    access: u64,
    count: usize,
) -> (MemoPosition, VirtAddr) {
    let Target::Nest(nest) = pend_info(token).target else {
        panic!("Nest")
    };
    let Snapshot::Nest(s) = inspect(Query::Nest { token, nest }) else {
        panic!("Nest")
    };
    let Value::Reservation {
        token,
        nest,
        reservation,
        address,
        ..
    } = mutate(Action::Reserve {
        token,
        nest,
        expected: s.version,
        reservation: ReserveRequest {
            placement,
            count: PageCount::new(count).unwrap(),
            level: CellLevel::new(0).unwrap(),
            access,
        },
    })
    else {
        panic!("Reservation")
    };
    (
        MemoPosition::new(token, nest, reservation, 0).unwrap(),
        address,
    )
}
pub fn insert(target: MemoPosition, slot: SlotPosition) {
    mutate(Action::Insert {
        target,
        expected: memo_status(target).generation,
        source: Source::Flat {
            position: slot,
            expected: slot_status(slot).version,
        },
    });
}
pub fn publish(memo: MemoPosition) {
    mutate(Action::Publish {
        memo,
        expected: memo_status(memo).generation,
        maintenance: None,
    });
}
pub fn retract(memo: MemoPosition) {
    crate::recovery::retract(memo);
}
pub fn block(start: SlotPosition) -> Maintenance {
    let Response::Reply(r) = call(Action::Block {
        start,
        count: 1,
        recover: None,
    }) else {
        panic!("Block")
    };
    let (Outcome::Committed(Value::Maintenance { block, .. })
    | Outcome::Pending(Value::Maintenance { block, .. })) = r.outcome()
    else {
        panic!("{:?}", r.outcome())
    };
    let m = Maintenance {
        token: start.token(),
        block,
    };
    let deadline = room::clock() + 2_000_000_000;
    loop {
        let Snapshot::Block(s) = inspect(Query::Block(m)) else {
            panic!("Block")
        };
        if s.phase == BlockPhase::Held {
            return m;
        }
        assert!(room::clock() < deadline);
        room::yield_();
    }
}

pub fn task_info(query: kernel_api::model::unit::UnitQuery) -> kernel_api::model::unit::UnitInfo {
    use kernel_api::model::unit::UnitInfo;
    let mut bytes = [0; UnitInfo::WIDTH];
    assert!(
        kernel_client::call::unit::inspect(query, VirtAddr::new(bytes.as_mut_ptr() as usize))
            .unwrap()
    );
    UnitInfo::from_bytes(&bytes).unwrap()
}
pub fn spawn(entry: extern "C" fn() -> !) -> TaskId {
    use kernel_api::model::unit::*;
    let bytes = UnitCreate::Task {
        team: None,
        start: crate::stack::Stack::new(entry as *const () as usize, &[], 65536).start,
    }
    .to_bytes();
    let CreatedUnit::Task(task) =
        ({
            let deadline = room::clock() + 5_000_000_000;
            loop {
                match kernel_client::call::unit::create(VirtAddr::new(bytes.as_ptr() as usize)) {
                    Ok(created) => break created,
                    Err(error) => assert_eq!(error.into_source(), kernel_api::abi::call::unit::UnitFail::Busy),
                }
                assert!(room::clock() < deadline, "Task admission stayed busy");
                room::yield_();
            }
        })
    else {
        panic!("Task")
    };
    task
}
pub fn join(task: TaskId) -> kernel_api::model::unit::UnitInfo {
    use kernel_api::model::unit::*;
    let end = room::clock() + 3_000_000_000;
    loop {
        let info = task_info(UnitQuery::Target(UnitTarget::Task(task)));
        if info.state == UnitState::Exited {
            return info;
        }
        assert!(room::clock() < end, "Task {task:?} stalled: {info:?}");
        room::yield_();
    }
}

pub fn accord(src: PendId, target: TaskId, limits: Limits) -> PendId {
    let bytes = AccordRequest {
        src,
        target: AccordTarget::Task(target),
        limits,
        mark: Mark::NONE,
    }
    .to_bytes();
    {
 let deadline = kernel_client::call::room::clock() + 5_000_000_000;
 loop {
 match pend::accord(VirtAddr::new(bytes.as_ptr() as usize)) {
 Ok(token) => break token,
 Err(error) => assert_eq!(error.into_source(), kernel_api::abi::call::pend::PendFail::Busy),
 }
 assert!(kernel_client::call::room::clock() < deadline);
 kernel_client::call::room::yield_();
 }
 }
}
