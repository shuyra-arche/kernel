//! Real User Tasks, controlled by the ordinary guest. No testing ABI is added.
use core::fmt::Write;
use kernel_api::model::unit::ExitScope;
use kernel_api::{
    abi::{
        call::{
            room::{RoomQuery, operations::Inspect},
            unit::UnitFail,
        },
        operation::RegisterCall,
        reply::SignReply,
    },
    address::{PageCount, VirtAddr},
    model::{
        cell::*,
        identity::{Mark, PendId, TaskId, TeamId},
        modifiable::Modifiable,
        nest::ReserveRequest,
        pend::{
            AccordRequest, AccordTarget, Distributable, Limits, NestRights, NestScope, Operations,
            PendRange, PendScope, ShareRights,
        },
        sign::SignRights,
        unit::{
            ControlId, CreatedUnit, ProgramKind, Schedule, ScheduleInfo, UnitCreate, UnitQuery,
            UnitState, UnitTarget,
        },
        wait::Wait,
    },
};
use kernel_client::call::{pend, room, share_view, sign, unit};
const fn call(class: usize, slot: usize) -> usize {
    ((kernel_api::abi::VERSION as usize) << 48) | (class << 32) | slot
}
core::arch::global_asm!(include_str!("worker.S"),
    collect=const call(7,18), trigger=const call(11,3), consume=const call(11,4),
    listen=const call(11,2), await_=const call(11,12), yield_=const call(0,0),
    clock=const call(0,4), wait_value=const call(0,2), inspect_room=const Inspect::SLOT,
    sleep=const call(0,1), exit=const call(1,5), reserve=const call(13,5),
    insert=const call(2,10), inspect=const call(2,7), publish=const call(2,4),
    pend_size=const kernel_api::model::pend::PendInfo::SIZE);
unsafe extern "C" {
    static realtime_worker_start: u8;
    static realtime_worker_end: u8;
}
const ADDRESS: usize = 0x40000;
fn limit() -> u64 {
    room::clock() + 5_000_000_000
}
fn state(id: TaskId) -> UnitState {
    crate::lifecycle::inspect(UnitQuery::Target(UnitTarget::Task(id))).state
}
fn wait_state(id: TaskId, expected: UnitState) {
    let end = limit();
    while state(id) != expected {
        assert!(
            room::clock() < end,
            "state {id:?}: {:?} != {expected:?}",
            state(id)
        );
        room::yield_();
    }
}
fn resume(id: TaskId) {
    let end = limit();
    loop {
        match unit::resume(UnitTarget::Task(id)) {
            Ok(()) => break,
            Err(e) => {
                assert_eq!(e.source, UnitFail::Busy);
                assert!(room::clock() < end);
                room::yield_();
            }
        }
    }
}
#[track_caller]
fn freeze(id: TaskId) {
    let end = limit();
    loop {
        match unit::freeze(UnitTarget::Task(id)) {
            Ok(()) => break,
            Err(e) => {
                assert_eq!(e.source, UnitFail::Busy);
                assert!(room::clock() < end, "freeze {id:?}: {:?}", state(id));
                room::yield_();
            }
        }
    }
    wait_state(id, UnitState::Frozen);
}
fn profile(id: TaskId) -> ScheduleInfo {
    let mut bytes = [0; ScheduleInfo::WIDTH];
    unit::profile(id, VirtAddr::new(bytes.as_mut_ptr() as usize)).unwrap();
    ScheduleInfo::from_bytes(&bytes).unwrap()
}
fn specification(priority: u64, ceiling: u64) -> Schedule {
    Schedule {
        hart: 1,
        priority,
        ceiling,
        // These cases verify dependency/lifecycle behavior, not a 2 ms WCET.
        // Kernel execution consumes credit too; debug/TCG entry overhead must
        // leave useful user execution in a period. The exhaustion and
        // preemption cases below override these values and keep their checks.
        budget: 100_000_000,
        period: 1_000_000_000,
        latency: 10_000_000,
        extensions: 0,
    }
}
fn admit(id: TaskId, spec: Schedule) -> Result<(), UnitFail> {
    unit::admit(id, crate::ptr(&spec.to_bytes())).map_err(|e| e.source)
}
fn endpoint() -> PendId {
    sign::create(crate::license::get()).unwrap()
}
fn grant(src: PendId, dst: TaskId, mark: u64, rights: SignRights) -> PendId {
    let bytes = AccordRequest {
        src,
        target: AccordTarget::Task(dst),
        mark: Mark::new(mark),
        limits: Limits {
            operations: Operations::Sign(rights),
            modifiable: Modifiable::No,
            distributable: Distributable::No,
            scope: PendScope::Object,
        },
    }
    .to_bytes();
    pend::accord(crate::ptr(&bytes)).unwrap()
}
#[track_caller]
fn event(token: PendId) {
    let SignReply::Triggered { sequence } = sign::listen(token, Wait::At(limit())).unwrap() else {
        panic!("missing event {token:?}")
    };
    sign::consume(token, sequence).unwrap();
}
fn close(token: PendId) {
    sign::delete(token).unwrap();
    crate::lifecycle::return_pend(token);
}
struct Fixture {
    team: TeamId,
    whole: PendId,
    execution: PendId,
    source: PendId,
    slot: SlotId,
    data: SlotId,
    shared: PendId,
}
impl Fixture {
    fn new() -> Self {
        let bytes = UnitCreate::Team {
            member_exit: false,
            nest: None,
            kind: ProgramKind::User,
            guard: ControlId::NONE,
        }
        .to_bytes();
        let CreatedUnit::Team { team, nest: whole } = unit::create(crate::ptr(&bytes)).unwrap()
        else {
            panic!("Team")
        };
        let slot = crate::allocate(1);
        let (mapping, address) = crate::attach(slot, 1);
        let start = core::ptr::addr_of!(realtime_worker_start);
        let len = core::ptr::addr_of!(realtime_worker_end) as usize - start as usize;
        assert!(len <= 4096);
        unsafe {
            core::ptr::copy_nonoverlapping(start, address.get() as *mut u8, len);
        }
        crate::retract(mapping, 0);
        crate::delete(crate::raw_memo(mapping, 0).object);
        let source = share_view::create(crate::license::get(), slot, Modifiable::No).unwrap();
        let (execution, _) = crate::space::reserve(
            whole,
            Placement::exact(VirtAddr::new(ADDRESS), 4096).unwrap(),
            1,
            CellLevel::BASE,
            10,
        );
        crate::handoff(crate::raw_memo(source, 0), crate::memo(execution, 0), false);
        crate::publish(execution, 0);
        let data = crate::allocate(1);
        let shared = share_view::create(crate::license::get(), data, Modifiable::Yes).unwrap();
        Self {
            team,
            whole,
            execution,
            source,
            slot,
            data,
            shared,
        }
    }
    #[track_caller]
    fn worker(
        &self,
        role: usize,
        timeout: u64,
        dependency: Option<PendId>,
        reply: Option<PendId>,
    ) -> Worker {
        let descriptor = ReserveRequest {
            placement: Placement::exact(VirtAddr::new(ADDRESS + 4096), 4096).unwrap(),
            count: PageCount::new(1).unwrap(),
            level: CellLevel::BASE,
            access: 6,
        }
        .to_bytes();
        let mut args = [0usize; 2 + ReserveRequest::WIDTH / 8];
        args[0] = role;
        args[1] = timeout as usize;
        for (word, bytes) in args[2..].iter_mut().zip(descriptor.chunks_exact(8)) {
            *word = usize::from_le_bytes(bytes.try_into().unwrap());
        }
        let bytes = UnitCreate::Task {
            team: Some(self.team),
            entry: VirtAddr::new(ADDRESS),
            args: VirtAddr::new(args.as_ptr() as usize),
            count: args.len() as u64,
            stack: VirtAddr::new(0),
        }
        .to_bytes();
        let CreatedUnit::Task(id) = unit::create(crate::ptr(&bytes)).unwrap() else {
            panic!("Task")
        };
        let prepared = endpoint();
        let gate = endpoint();
        let done = endpoint();
        grant(prepared, id, 1, SignRights::TRIGGER);
        grant(gate, id, 2, SignRights::LISTEN | SignRights::CONSUME);
        if let Some(token) = dependency {
            grant(token, id, 3, SignRights::LISTEN);
        }
        let reply_grant = reply.map(|token| grant(token, id, 4, SignRights::TRIGGER));
        grant(done, id, 5, SignRights::TRIGGER);
        if role == 7 {
            let descriptor = AccordRequest {
                src: self.shared,
                target: AccordTarget::Task(id),
                mark: Mark::new(6),
                limits: Limits {
                    operations: Operations::Share(ShareRights::ASSOCIATE),
                    modifiable: Modifiable::Yes,
                    distributable: Distributable::No,
                    scope: PendScope::Pages(PendRange::Inherit),
                },
            }
            .to_bytes();
            pend::accord(crate::ptr(&descriptor)).unwrap();
            let descriptor = AccordRequest {
                src: self.whole,
                target: AccordTarget::Task(id),
                mark: Mark::new(7),
                limits: Limits {
                    operations: Operations::Nest(NestRights::INSPECT | NestRights::RESERVE),
                    modifiable: Modifiable::Yes,
                    distributable: Distributable::No,
                    scope: PendScope::Nest(NestScope::Whole),
                },
            }
            .to_bytes();
            pend::accord(crate::ptr(&descriptor)).unwrap();
        }
        resume(id);
        event(prepared);
        freeze(id);
        close(prepared);
        Worker {
            id,
            gate,
            done,
            reply_grant,
        }
    }
    fn close(self) {
        unit::finish(self.team).unwrap();
        let end = limit();
        loop {
            match unit::delete(UnitTarget::Team(self.team)) {
                Ok(()) => break,
                Err(e) => {
                    assert_eq!(e.source, UnitFail::Busy);
                    assert!(room::clock() < end);
                    room::yield_();
                }
            }
        }
        crate::lifecycle::return_pend(self.execution);
        crate::lifecycle::return_pend(self.whole);
        crate::lifecycle::return_pend(self.shared);
        crate::delete(CellObject::Slot(self.data));
        crate::lifecycle::return_pend(self.source);
        crate::delete(CellObject::Slot(self.slot));
    }
}
struct Worker {
    id: TaskId,
    gate: PendId,
    done: PendId,
    reply_grant: Option<PendId>,
}
impl Worker {
    fn start(&self) {
        sign::trigger(self.gate).unwrap();
        resume(self.id);
    }
    fn stop(&self) {
        let end = limit();
        loop {
            match sign::trigger(self.gate) {
                Ok(()) => break,
                Err(error) => {
                    assert_eq!(error.source, kernel_api::abi::call::sign::SignFail::Busy);
                    assert!(room::clock() < end);
                    room::yield_();
                }
            }
        }
    }
    #[track_caller]
    fn join(self, value: usize) {
        event(self.done);
        self.finish(value);
    }
    fn finish(self, value: usize) {
        assert!(crate::lifecycle::wait_task(self.id));
        assert_eq!(crate::lifecycle::result(self.id), (1, value));
        crate::lifecycle::delete(self.id);
        close(self.gate);
        close(self.done);
    }
    fn cancel(self) {
        unit::stop(UnitTarget::Task(self.id)).unwrap();
        assert!(crate::lifecycle::wait_task(self.id));
        crate::lifecycle::delete(self.id);
        close(self.gate);
        close(self.done);
    }
    fn fault(self) {
        assert!(crate::lifecycle::wait_task(self.id));
        let info = crate::lifecycle::inspect(UnitQuery::Target(UnitTarget::Task(self.id)));
        assert_eq!(
            info.exit.unwrap().0,
            kernel_api::model::unit::ExitCause::Fault
        );
        crate::lifecycle::delete(self.id);
        close(self.gate);
        close(self.done);
    }
}
pub(super) fn run() {
    let fixture = Fixture::new();
    let worker = fixture.worker(1, 0, None, None);
    let mut spec = specification(5, 5);
    spec.budget = 20_000_000;
    spec.period = 1_000_000_000;
    match admit(worker.id, spec) {
        Err(UnitFail::Denied) => {
            // The harness verifies this branch only on the single-Hart image.
            assert_eq!(room::inspect(RoomQuery::Ordinary).unwrap(), 1);
            unit::stop(UnitTarget::Task(worker.id)).unwrap();
            assert!(crate::lifecycle::wait_task(worker.id));
            crate::lifecycle::delete(worker.id);
            close(worker.gate);
            close(worker.done);
            fixture.close();
            let _ = writeln!(crate::Console, "PASS realtime single-hart=denied");
            return;
        }
        other => assert_eq!(other, Ok(())),
    }
    assert_eq!(unit::evict(worker.id).unwrap_err().source, UnitFail::Denied);
    let supervisor = crate::lifecycle::spawn(supervisor_exit as *const () as usize);
    assert_eq!(admit(supervisor, spec), Err(UnitFail::Denied));
    resume(supervisor);
    assert!(crate::lifecycle::wait_task(supervisor));
    crate::lifecycle::delete(supervisor);
    worker.start();
    event(worker.done); // acknowledged after an actual admitted Yield
    let end = limit();
    let exhausted = loop {
        let p = profile(worker.id);
        if p.remaining == 0 {
            break p;
        }
        assert!(room::clock() < end);
        room::yield_();
    };
    assert!(exhausted.consumed >= spec.budget);
    freeze(worker.id);
    let frozen = profile(worker.id);
    assert_eq!(frozen.remaining, 0);
    assert_eq!(admit(worker.id, spec), Err(UnitFail::Denied));
    resume(worker.id);
    let end = limit();
    loop {
        let p = profile(worker.id);
        if p.replenishment > exhausted.replenishment && p.consumed > exhausted.consumed {
            break;
        }
        assert!(room::clock() < end);
        room::yield_();
    }
    worker.stop();
    worker.join(100);
    ordinary_capacity(&fixture, spec);
    let _ = writeln!(
        crate::Console,
        "PASS realtime budget=1 replenishment=1 yield=1 freeze=1 supervisor=denied"
    );
    preemption(&fixture);
    dependencies(&fixture);
    for (role, extensions, fault) in [(3, 0, false), (4, 0, true), (5, 0, true), (5, 1, false)] {
        let worker = fixture.worker(role, 0, None, None);
        let mut spec = specification(5, 5);
        spec.extensions = extensions;
        admit(worker.id, spec).unwrap();
        worker.start();
        if fault {
            worker.fault()
        } else {
            worker.join(if role == 3 { 103 } else { 105 });
        }
    }
    fixture.close();
    let _ = writeln!(
        crate::Console,
        "PASS realtime resident=1 restricted=1 fault=1 prepared-float=1 cleanup=1"
    );
}

fn ordinary_capacity(fixture: &Fixture, spec: Schedule) {
    let first = fixture.worker(1, 0, None, None);
    let second = fixture.worker(1, 0, None, None);
    let capacity = || room::inspect(RoomQuery::Ordinary).unwrap();
    let before = capacity();
    assert!(before > 1);
    admit(first.id, spec).unwrap();
    assert_eq!(capacity(), before - 1);
    admit(second.id, spec).unwrap();
    assert_eq!(capacity(), before - 1);
    first.cancel();
    assert_eq!(capacity(), before - 1);
    second.cancel();
    assert_eq!(capacity(), before);
    writeln!(crate::Console, "PASS room-inspect ordinary={before}").unwrap();
}
extern "C" fn supervisor_exit() -> ! {
    unit::exit(0, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
fn dependencies(fixture: &Fixture) {
    let reply_a = endpoint();
    let reply_b = endpoint();
    let server_b = fixture.worker(0, 0, None, Some(reply_b));
    let server_a = fixture.worker(2, 0, Some(reply_b), Some(reply_a));
    let medium = fixture.worker(0, 0, None, None);
    let caller = fixture.worker(2, 0, Some(reply_a), None);
    for (id, priority, ceiling) in [
        (server_b.id, 1, 9),
        (server_a.id, 3, 9),
        (medium.id, 5, 5),
        (caller.id, 9, 9),
    ] {
        admit(id, specification(priority, ceiling)).unwrap();
    }
    sign::assign(reply_b, server_b.id, server_b.reply_grant.unwrap()).unwrap();
    sign::assign(reply_a, server_a.id, server_a.reply_grant.unwrap()).unwrap();
    server_b.start();
    medium.start();
    server_a.start();
    wait_state(server_a.id, UnitState::Waiting);
    caller.start();
    wait_state(caller.id, UnitState::Waiting);
    assert_eq!(profile(server_a.id).effective, 9);
    assert_eq!(profile(server_b.id).effective, 9);
    assert_eq!(profile(medium.id).effective, 5);
    server_b.stop();
    caller.join(209);
    server_a.join(209);
    server_b.join(100);
    medium.stop();
    medium.join(100);
    close(reply_a);
    close(reply_b);
    for cancel in 0..5 {
        let reply = endpoint();
        let server = fixture.worker(0, 0, None, Some(reply));
        let caller = fixture.worker(
            2,
            if cancel == 0 { 1_000_000_000 } else { 0 },
            Some(reply),
            None,
        );
        admit(server.id, specification(1, 9)).unwrap();
        admit(caller.id, specification(9, 9)).unwrap();
        sign::assign(reply, server.id, server.reply_grant.unwrap()).unwrap();
        server.start();
        caller.start();
        wait_state(caller.id, UnitState::Waiting);
        assert_eq!(profile(server.id).effective, 9);
        if cancel == 1 {
            sign::resign(reply).unwrap();
            sign::assign(reply, server.id, server.reply_grant.unwrap()).unwrap();
        }
        if cancel == 2 {
            let end = limit();
            loop {
                match pend::revoke(server.id, server.reply_grant.unwrap()) {
                    Ok(()) => break,
                    Err(error) => {
                        assert_eq!(error.source, kernel_api::abi::call::pend::PendFail::Busy);
                        assert!(room::clock() < end);
                        room::yield_();
                    }
                }
            }
        }
        if cancel == 3 {
            server.cancel();
            caller.join(198);
            close(reply);
            continue;
        }
        if cancel == 4 {
            caller.cancel();
        } else {
            caller.join(if cancel == 0 { 200 } else { 199 });
        }
        assert_eq!(profile(server.id).effective, 1);
        if cancel == 2 {
            // Its revoked Trigger grant must fail, not deliver a stale reply.
            server.stop();
            assert!(crate::lifecycle::wait_task(server.id));
            assert_eq!(crate::lifecycle::result(server.id), (1, 255));
            crate::lifecycle::delete(server.id);
            close(server.gate);
            close(server.done);
        } else {
            server.stop();
            server.join(100);
        }
        close(reply);
    }
    rejection(fixture);
    let _ = writeln!(
        crate::Console,
        "PASS realtime nested=2 inheritance=1 timeout=1 resign=1 revoke=1 server-exit=1 caller-stop=1 cycle=1 ceiling=1"
    );
}

fn preemption(fixture: &Fixture) {
    let low = fixture.worker(7, 0, None, None);
    let high = fixture.worker(6, 0, None, None);
    let mut slow = specification(1, 1);
    slow.budget = 4_000_000_000;
    slow.period = 10_000_000_000;
    let mut fast = specification(9, 9);
    fast.period = 10_000_000_000;
    fast.budget = 1_000_000_000;
    admit(low.id, slow).unwrap();
    admit(high.id, fast).unwrap();
    let excess = fixture.worker(0, 0, None, None);
    let mut overload = slow;
    overload.priority = 10;
    overload.ceiling = 10;
    overload.budget = 9_990_000_000;
    assert_eq!(admit(excess.id, overload), Err(UnitFail::Busy));
    assert_eq!(profile(low.id).schedule, slow);
    unit::stop(UnitTarget::Task(excess.id)).unwrap();
    assert!(crate::lifecycle::wait_task(excess.id));
    crate::lifecycle::delete(excess.id);
    close(excess.gate);
    close(excess.done);
    high.start();
    wait_state(high.id, UnitState::Waiting);
    low.start();
    event(low.done);
    room::sleep_until(room::clock() + 5_000_000).unwrap();
    assert!(profile(high.id).remaining > 0);
    let end = limit();
    let before = loop {
        let p = profile(low.id);
        if p.consumed >= 1_000_000 {
            break p;
        }
        assert!(room::clock() < end);
        room::yield_();
    };
    high.stop();
    event(high.done);
    let after = profile(low.id);
    assert_eq!(before.replenishment, after.replenishment);
    assert!(
        after.remaining > 0,
        "higher priority waited for budget exhaustion: before={before:?}, after={after:?}"
    );
    high.finish(106);
    low.stop();
    low.join(100);
    let _ = writeln!(
        crate::Console,
        "PASS realtime preemption=1 cross-hart=1 overload=denied"
    );
}

fn rejection(fixture: &Fixture) {
    let reply = endpoint();
    let server = fixture.worker(0, 0, None, Some(reply));
    let caller = fixture.worker(2, 0, Some(reply), None);
    admit(server.id, specification(1, 3)).unwrap();
    admit(caller.id, specification(9, 9)).unwrap();
    sign::assign(reply, server.id, server.reply_grant.unwrap()).unwrap();
    server.start();
    caller.start();
    caller.join(199);
    assert_eq!(profile(server.id).effective, 1);
    server.stop();
    server.join(100);
    close(reply);
    let one = endpoint();
    let two = endpoint();
    let a = fixture.worker(2, 0, Some(two), Some(one));
    let b = fixture.worker(2, 0, Some(one), Some(two));
    admit(a.id, specification(5, 9)).unwrap();
    admit(b.id, specification(5, 9)).unwrap();
    sign::assign(one, a.id, a.reply_grant.unwrap()).unwrap();
    sign::assign(two, b.id, b.reply_grant.unwrap()).unwrap();
    a.start();
    wait_state(a.id, UnitState::Waiting);
    b.start();
    b.join(199);
    a.join(209);
    close(one);
    close(two);
}
