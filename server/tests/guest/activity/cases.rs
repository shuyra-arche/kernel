//! Authorization, identity, aliases and real simultaneous user writers.
use super::*;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::*};

fn page() -> (
    kernel_api::model::flat::SlotPosition,
    MemoPosition,
    VirtAddr,
) {
    let slot = create(1);
    mutate(Action::Occupy {
        slot,
        expected: slot_status(slot).version,
    });
    let (memo, address) = reserve(space());
    insert(memo, slot);
    publish(memo);
    (slot, memo, address)
}
fn after(reader: &Reader, boundary: u64) -> Sample {
    loop {
        let s = reader.next();
        if s.end_ns >= boundary {
            return s;
        }
    }
}
fn aliases() {
    let (slot, memo, address) = page();
    let (alias, _) = reserve(space());
    insert(alias, slot);
    publish(alias);
    let Value::CreatedNest { token: foreign, .. } =
        mutate(Action::CreateNest { license: license() })
    else {
        panic!("Nest")
    };
    let (other, _) = reserve(foreign);
    insert(other, slot);
    publish(other);
    let a = Reader::open(memo);
    let b = Reader::open(alias);
    let c = Reader::open(other);
    a.expect(Evidence::NotObserved, Evidence::NotObserved);
    b.expect(Evidence::NotObserved, Evidence::NotObserved);
    c.expect(Evidence::NotObserved, Evidence::NotObserved);
    let at = room::clock();
    unsafe { (address.get() as *mut u64).write_volatile(7) };
    let hit = a.expect(Evidence::Observed, Evidence::Observed);
    for reader in [&b, &c] {
        let idle = after(reader, at + sampling::MIN_PERIOD_NS * 2);
        assert_eq!(
            (idle.accessed, idle.dirty),
            (Evidence::NotObserved, Evidence::NotObserved)
        );
        assert_eq!(hit.page, idle.page);
        assert_ne!((hit.nest, hit.reservation), (idle.nest, idle.reservation));
    }
    let old = memo_status(memo).generation;
    retract(memo);
    assert_eq!(derive(memo, AccordTarget::Caller), Err(ShareFail::Busy));
    let end = room::clock() + 8_000_000_000;
    loop {
        let s = a.next();
        if s.validity == Validity::Stale {
            assert_eq!(s.generation, old.get());
            assert_eq!(
                (s.accessed, s.dirty),
                (Evidence::Unknown, Evidence::Unknown)
            );
            break;
        }
        assert!(room::clock() < end);
    }
    mutate(Action::Remove {
        memo,
        expected: memo_status(memo).generation,
    });
    let replacement = create(1);
    mutate(Action::Occupy {
        slot: replacement,
        expected: slot_status(replacement).version,
    });
    insert(memo, replacement);
    publish(memo);
    let fresh = Reader::open(memo);
    let current = fresh.expect(Evidence::NotObserved, Evidence::NotObserved);
    assert!(current.generation > old.get());
    assert_ne!(current.page, hit.page);
    assert!(sign::listen(a.event, Wait::Poll).is_ok()); // Terminal is not republished after ACK.
    for reader in [&a, &b, &c, &fresh] {
        reader.cancel();
    }
    let _ = writeln!(
        Console,
        "PASS activity aliases=3 nests=2 stale=1 replacement=1 cancel=1"
    );
}
fn authority() {
    let slot = create(1);
    mutate(Action::Occupy {
        slot,
        expected: slot_status(slot).version,
    });
    let (root, address) = reserve_pages(space(), Placement::lowest(4096).unwrap(), 6, 2);
    let second = MemoPosition::new(root.token(), root.nest(), root.reservation(), 1).unwrap();
    insert(second, slot);
    publish(second);
    let parent = match task_info(kernel_api::model::unit::UnitQuery::Caller).target {
        kernel_api::model::unit::UnitTarget::Task(id) => id,
        _ => panic!("Task"),
    };
    let mut limits = pend_info(root.token()).limits;
    limits.operations = Operations::Nest(NestRights::INSPECT | NestRights::SAMPLE);
    limits.modifiable = kernel_api::model::modifiable::Modifiable::No;
    limits.distributable = Distributable::No;
    limits.scope = PendScope::Nest(NestScope::Reservation {
        id: root.reservation(),
        range: PendRange::Pages {
            offset: 1,
            count: 1,
        },
    });
    let selected = accord(root.token(), parent, limits);
    let outside = MemoPosition::new(selected, root.nest(), root.reservation(), 0).unwrap();
    assert_eq!(observe(outside), Err(ShareFail::Denied));
    let inside = MemoPosition::new(selected, root.nest(), root.reservation(), 1).unwrap();
    let reader = Reader::open(inside);
    let sample = reader.expect(Evidence::NotObserved, Evidence::NotObserved);
    assert_eq!(sample.position, 1);
    assert_eq!(
        unsafe { ((address.get() + 4096) as *const u64).read_volatile() },
        0
    );
    reader.expect(Evidence::Observed, Evidence::NotObserved);
    let request = AccordRequest {
        src: PendId::from_bytes(&0x100000u64.to_le_bytes()).unwrap(),
        target: AccordTarget::Caller,
        limits: sampling::limits(1).unwrap(),
        mark: Mark::NONE,
    }
    .to_bytes();
    assert_eq!(
        share_view::derive(license(), VirtAddr::new(request.as_ptr() as usize))
            .map_err(|e| e.source),
        Err(ShareFail::Denied)
    );
    reader.cancel();
    let _ = writeln!(
        Console,
        "PASS activity window=1 forged=1 inspect-alone=1 sparse-view=1 pair-order=sign-share"
    );
}
fn capacity() {
    let (_, memo, _) = page();
    let end = room::clock() + 200_000_000;
    while room::clock() < end {
        pause();
    }
    let mut tokens = [None; sampling::MAX_SUBSCRIPTIONS];
    let deadline = room::clock() + 8_000_000_000;
    for out in &mut tokens {
        *out = Some(loop {
            match observe(memo) {
                Ok(pair) => break pair,
                Err(ShareFail::Busy) => pause(),
                Err(e) => panic!("capacity {e:?}"),
            }
            assert!(room::clock() < deadline, "cancelled slot did not settle");
        });
    }
    let deadline = room::clock() + 8_000_000_000;
    for (event, _) in tokens.iter().flatten() {
        while !matches!(
            sign::listen(*event, Wait::Poll).unwrap(),
            kernel_api::abi::reply::SignReply::Triggered { .. }
        ) {
            assert!(room::clock() < deadline, "subscription fairness");
            pause();
        }
    }
    assert_eq!(observe(memo), Err(ShareFail::Busy));
    for (event, _) in tokens.into_iter().flatten() {
        pend::return_(event).unwrap();
    }
    let r = Reader::open(memo);
    r.expect(Evidence::NotObserved, Evidence::NotObserved);
    r.cancel();
    let _ = writeln!(Console, "PASS activity capacity=16 overload=1 reuse=1");
}
static ADDRESS: AtomicUsize = AtomicUsize::new(0);
static GO: AtomicBool = AtomicBool::new(false);
static STARTED: AtomicUsize = AtomicUsize::new(0);
static WRITES: AtomicU64 = AtomicU64::new(0);
extern "C" fn writer() -> ! {
    let pointer = ADDRESS.load(Acquire) as *const AtomicU64;
    STARTED.fetch_add(1, AcqRel);
    while GO.load(Acquire) {
        for _ in 0..256 {
            unsafe { &*pointer }.fetch_add(1, Relaxed);
        }
        WRITES.fetch_add(256, Relaxed);
        room::yield_();
    }
    exit(0)
}
fn throughput(duration: u64) -> (u64, u64) {
    let start = room::clock();
    let before = WRITES.load(Acquire);
    while room::clock() - start < duration {
        pause();
    }
    (WRITES.load(Acquire) - before, room::clock() - start)
}
fn writers() {
    let (_, memo, address) = page();
    ADDRESS.store(address.get(), Release);
    GO.store(true, Release);
    let workers = [spawn(writer), spawn(writer), spawn(writer)];
    for worker in workers {
        unit::resume(worker.into()).unwrap();
    }
    let end = room::clock() + 8_000_000_000;
    while STARTED.load(Acquire) != 3 {
        assert!(room::clock() < end);
        pause();
    }
    let (off, off_ns) = throughput(500_000_000);
    let reader = Reader::open(memo);
    reader.expect(Evidence::Observed, Evidence::Observed);
    let begin = room::clock();
    let before = WRITES.load(Acquire);
    let mut samples = 0;
    let deadline = begin + 8_000_000_000;
    let mut previous = 0;
    while room::clock() - begin < 500_000_000 || samples < 3 {
        assert!(room::clock() < deadline, "three completed writer epochs");
        let s = reader.next();
        assert_eq!(s.validity, Validity::Observed);
        assert!(s.epoch > previous);
        previous = s.epoch;
        assert_eq!(s.dirty, Evidence::Observed);
        samples += 1;
    }
    let on = WRITES.load(Acquire) - before;
    let on_ns = room::clock() - begin;
    reader.cancel();
    GO.store(false, Release);
    for worker in workers {
        join(worker);
        let _ = writeln!(Console, "TRACE activity-writer task={}", worker.get());
    }
    assert!(off > 0 && on > 0 && samples >= 3);
    let _ = writeln!(
        Console,
        "PASS activity writers=3 samples={samples} off_writes={off} off_ns={off_ns} on_writes={on} on_ns={on_ns}"
    );
}
pub fn run() {
    aliases();
    authority();
    safety();
    consumer_exit();
    large_leaf();
    capacity();
    writers();
}

static FAULT_ADDRESS: AtomicUsize = AtomicUsize::new(0);
extern "C" fn read_withdrawn() -> ! {
    let _ = unsafe { (FAULT_ADDRESS.load(Acquire) as *const u64).read_volatile() };
    exit(77)
}
extern "C" fn write_readonly() -> ! {
    unsafe { (FAULT_ADDRESS.load(Acquire) as *mut u64).write_volatile(77) };
    exit(77)
}
fn fault(entry: extern "C" fn() -> !, address: VirtAddr) {
    FAULT_ADDRESS.store(address.get(), Release);
    let task = spawn(entry);
    unit::resume(task.into()).unwrap();
    assert!(matches!(
        join(task).exit,
        Some((kernel_api::model::unit::ExitCause::Fault, _))
    ));
}
pub fn withdrawn(mapping: MemoPosition, address: VirtAddr) {
    let end = room::clock() + 8_000_000_000;
    while memo_status(mapping).phase != MemoPhase::Empty {
        assert!(room::clock() < end, "output withdrawal");
        pause();
    }
    fault(read_withdrawn, address);
}
fn safety() {
    let (_, memo, _) = page();
    let reader = Reader::open(memo);
    let snapshot = reader.expect(Evidence::NotObserved, Evidence::NotObserved);
    fault(write_readonly, reader.address);
    assert_eq!(
        unsafe { &*(reader.address.get() as *const SamplePage) }
            .read()
            .unwrap()
            .page,
        snapshot.page
    );
    let Target::View(ViewTarget::Share(view)) = pend_info(reader.data).target else {
        panic!("View")
    };
    let (copy, copy_address) = reserve_at(space(), Placement::lowest(4096).unwrap(), 2);
    mutate(Action::Insert {
        target: copy,
        expected: memo_status(copy).generation,
        source: Source::Share {
            token: reader.data,
            view,
            page: 0,
            count: 1,
        },
    });
    publish(copy);
    let (rw, _) = reserve(space());
    mutate(Action::Insert {
        target: rw,
        expected: memo_status(rw).generation,
        source: Source::Share {
            token: reader.data,
            view,
            page: 0,
            count: 1,
        },
    });
    let kernel_client::memory::Response::Reply(reply) = call(Action::Publish {
        memo: rw,
        expected: memo_status(rw).generation,
        maintenance: None,
    }) else {
        panic!("reply")
    };
    assert_eq!(reply.outcome(), Outcome::Rejected(Failure::Denied));
    reader.cancel();
    withdrawn(reader.mapping, reader.address);
    withdrawn(copy, copy_address);
    let (again, _) = reserve_at(space(), Placement::lowest(4096).unwrap(), 2);
    let kernel_client::memory::Response::Reply(reply) = call(Action::Insert {
        target: again,
        expected: memo_status(again).generation,
        source: Source::Share {
            token: reader.data,
            view,
            page: 0,
            count: 1,
        },
    }) else {
        panic!("reply")
    };
    assert_eq!(reply.outcome(), Outcome::Rejected(Failure::Denied));
    let _ = writeln!(
        Console,
        "PASS activity readonly=1 write-upgrade-denied=1 physical-withdrawal=1 reuse-denied=1"
    );
}
static CHILD_VIEWS: [AtomicUsize; 2] = [const { AtomicUsize::new(0) }; 2];
static CHILD_SPACE: AtomicUsize = AtomicUsize::new(0);
static CHILD_SORTED: AtomicBool = AtomicBool::new(false);
static CHILD_REVOKED: AtomicBool = AtomicBool::new(false);
static CHILD_READY: AtomicBool = AtomicBool::new(false);
static CHILD_WRITE: AtomicU64 = AtomicU64::new(0);
extern "C" fn consumer() -> ! {
    let namespace = PendId::from_bytes(&(CHILD_SPACE.load(Acquire) as u64).to_le_bytes()).unwrap();
    let views = CHILD_VIEWS
        .each_ref()
        .map(|word| PendId::from_bytes(&(word.load(Acquire) as u64).to_le_bytes()).unwrap());
    let cold = Reader::from_pair(pair(views[0]), namespace);
    let hot = Reader::from_pair(pair(views[1]), namespace);
    cold.expect(Evidence::NotObserved, Evidence::NotObserved);
    hot.expect(Evidence::NotObserved, Evidence::NotObserved);
    CHILD_READY.store(true, Release);
    let end = room::clock() + 8_000_000_000;
    while CHILD_WRITE.load(Acquire) == 0 {
        assert!(room::clock() < end);
        pause();
    }
    let accessed = hot.expect(Evidence::Observed, Evidence::Observed);
    let idle = after(
        &cold,
        CHILD_WRITE.load(Acquire) + 2 * sampling::MIN_PERIOD_NS,
    );
    assert_eq!(idle.dirty, Evidence::NotObserved);
    let mut candidates = [idle, accessed];
    candidates.sort_unstable_by_key(|s| {
        core::cmp::Reverse((
            s.dirty == Evidence::Observed,
            s.accessed == Evidence::Observed,
        ))
    });
    assert_eq!(candidates[0].page, accessed.page);
    assert_ne!(accessed.page, idle.page);
    CHILD_SORTED.store(true, Release);
    let deadline = room::clock() + 8_000_000_000;
    while !CHILD_REVOKED.load(Acquire) {
        assert!(room::clock() < deadline);
        pause();
    }
    assert!(sign::listen(hot.event, Wait::Poll).is_err());
    assert_eq!(pend_info(hot.data).status, PendStatus::Dead);
    while memo_status(hot.mapping).phase != MemoPhase::Empty {
        assert!(room::clock() < deadline);
        pause();
    }
    // The consumer chooses a ranking; this does not authorize eviction/reuse.
    // Leave both output mappings, grants and an unread snapshot to Task exit.
    let _ = cold.pending();
    exit(0)
}
fn consumer_exit() {
    let (_, cold, _) = page();
    let (_, hot, address) = page();
    let child = spawn(consumer);
    let parent = match task_info(kernel_api::model::unit::UnitQuery::Caller).target {
        kernel_api::model::unit::UnitTarget::Task(id) => id,
        _ => panic!("Task"),
    };
    let mut control = pend_info(hot.token()).limits;
    control.operations = Operations::Nest(NestRights::INSPECT | NestRights::SAMPLE);
    control.modifiable = kernel_api::model::modifiable::Modifiable::No;
    control.distributable = Distributable::No;
    let hot_token = accord(hot.token(), parent, control);
    let hot_selected =
        MemoPosition::new(hot_token, hot.nest(), hot.reservation(), hot.page()).unwrap();
    for (position, word) in [cold, hot_selected].into_iter().zip(CHILD_VIEWS.iter()) {
        let end = room::clock() + 8_000_000_000;
        let token = loop {
            match derive(position, AccordTarget::Task(child)) {
                Ok(token) => break token,
                Err(ShareFail::Busy) => pause(),
                Err(e) => panic!("derive recipient {e:?}"),
            }
            assert!(room::clock() < end);
        };
        word.store(token.get(), Release);
    }
    let mut namespace = pend_info(space()).limits;
    namespace.operations = Operations::Nest(NestRights::INSPECT | NestRights::RESERVE);
    namespace.modifiable = kernel_api::model::modifiable::Modifiable::No;
    namespace.distributable = Distributable::No;
    CHILD_SPACE.store(accord(space(), child, namespace).get(), Release);
    unit::resume(child.into()).unwrap();
    let end = room::clock() + 8_000_000_000;
    while !CHILD_READY.load(Acquire) {
        assert!(room::clock() < end);
        pause();
    }
    unsafe { (address.get() as *mut u64).write_volatile(121) };
    CHILD_WRITE.store(room::clock(), Release);
    let deadline = room::clock() + 8_000_000_000;
    while !CHILD_SORTED.load(Acquire) {
        assert!(room::clock() < deadline);
        pause();
    }
    loop {
        match pend::revoke(parent, hot_token) {
            Ok(()) => break,
            Err(e) => assert_eq!(e.source, kernel_api::abi::call::pend::PendFail::Busy),
        }
        assert!(room::clock() < deadline);
        pause();
    }
    CHILD_REVOKED.store(true, Release);
    assert_eq!(join(child).result, Some(0));
    // Capacity below must still admit all 16 entries after this consumer exits.
    let _ = writeln!(
        Console,
        "PASS activity consumer-exit=1 restricted-readonly=1 unread=1 sorted-candidates=2 ancestor-revoke=1"
    );
}
fn large_leaf() {
    let level = CellLevel::new(1).unwrap();
    let Value::CreatedFlat { token, flat, .. } = mutate(Action::CreateFlat {
        license: license(),
        count: 512,
        level,
    }) else {
        panic!("Flat")
    };
    let slot = kernel_api::model::flat::SlotPosition::new(token, flat, 0).unwrap();
    mutate(Action::Occupy {
        slot,
        expected: slot_status(slot).version,
    });
    let (memo, _) = crate::reservation::reserve(
        space(),
        ReserveRequest {
            placement: Placement::lowest(2 * 1024 * 1024).unwrap(),
            count: PageCount::new(512).unwrap(),
            level,
            access: 6,
        },
    );
    insert(memo, slot);
    publish(memo);
    assert_eq!(observe(memo), Err(ShareFail::Unsupported));
    let _ = writeln!(
        Console,
        "PASS activity large-unsupported=1 geometry-preserved=1"
    );
}

pub fn stopped(slot: kernel_api::model::flat::SlotPosition, memo: MemoPosition, address: VirtAddr) {
    let reader = Reader::open(memo);
    let end = room::clock() + 200_000_000;
    while room::clock() < end {
        pause();
    }
    assert!(
        unsafe { &*(reader.address.get() as *const SamplePage) }
            .read()
            .is_none()
    );
    assert!(matches!(
        sign::listen(reader.event, Wait::Poll).unwrap(),
        kernel_api::abi::reply::SignReply::Pending
    ));
    unsafe { (address.get() as *mut u64).write_volatile(93) };
    assert_eq!(unsafe { (address.get() as *const u64).read_volatile() }, 93);
    retract(memo);
    let held = block(slot);
    mutate(Action::Vacate {
        slot,
        expected: slot_status(slot).version,
        maintenance: held,
    });
    mutate(Action::Allow { maintenance: held });
    assert_eq!(
        slot_status(slot).phase,
        kernel_api::model::flat::SlotPhase::Missing
    );
    // Explicit source cleanup still retires its mappings without the observer.
    let end = room::clock() + 8_000_000_000;
    loop {
        match pend::return_(reader.data) {
            Ok(()) => break,
            Err(e) => assert_eq!(e.source, kernel_api::abi::call::pend::PendFail::Busy),
        }
        assert!(room::clock() < end);
        pause();
    }
    withdrawn(reader.mapping, reader.address);
    let (_, _, fresh) = page();
    assert_eq!(unsafe { (fresh.get() as *const u64).read_volatile() }, 0);
    let _ = writeln!(
        Console,
        "PASS activity observer-stopped=1 retained-pending=1 ordinary-retirement=1 output-return=1 allocation=1"
    );
}
