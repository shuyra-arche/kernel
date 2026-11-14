use super::super::{
    Allocation, Flat, FlatError, FlatSource, Slot,
    storage::materialize::{Reservation, Step},
    transfer::{Transfer, TransferError},
};
use super::*;
use crate::basis::{
    cell::{
        self,
        page::{Owner, Page},
    },
    unit::task::registry,
};
use crate::metal::sync::SpinLock;
use alloc::{sync::Arc, vec::Vec};
use core::{
    alloc::{AllocError, Allocator, Layout},
    ptr::NonNull,
    time::Duration,
};
use kernel_api::{
    address::{PageIndex, PageOffset},
    model::{
        cell::CellLevel,
        identity::{TaskId, TeamId},
    },
};

const GRAIN: usize = 512;
const COOKIE: u64 = 0x1030_cafe_0123_4567;
fn level() -> CellLevel {
    CellLevel::new(1).unwrap()
}
fn flat(owner: Owner, cells: usize) -> Arc<Flat> {
    Flat::acquire(owner, Allocation::at_level(cells * GRAIN, level()).unwrap()).unwrap()
}
fn until() -> u64 {
    now().saturating_add(clock::duration_to_ticks(Duration::from_secs(30)))
}
fn wait(mut predicate: impl FnMut() -> bool) {
    let deadline = until();
    while !predicate() {
        assert!(now() < deadline, "Flat probe stalled");
        yield_now();
    }
}
fn reserve(flat: &Flat) -> Reservation<'_> {
    match Reservation::prepare(&flat.state, PageIndex::ZERO, GRAIN, level()).unwrap() {
        Step::Allocate(reservation) => reservation,
        Step::Ready(_) => panic!("expected Fresh"),
    }
}
fn resident(flat: &Flat, index: usize) -> (usize, usize) {
    let guard = flat.state.lock();
    let FlatSource::Managed(slots) = &guard.source else {
        unreachable!()
    };
    let (_, Slot::Resident(page)) = slots.iter().find(|(at, _)| at.get() == index).unwrap() else {
        panic!("not Resident")
    };
    (page.id().get(), page.physical_address())
}

// This allocator never supplies fake storage: it forwards the actual RV64
// frame allocator, recording exact successful returns of unpublished Pages.
struct Counted {
    takes: AtomicUsize,
    gives: AtomicUsize,
}
static COUNTED: Counted = Counted {
    takes: AtomicUsize::new(0),
    gives: AtomicUsize::new(0),
};
unsafe impl Allocator for Counted {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let block = crate::metal::allocator::frame::instance().allocate(layout)?;
        self.takes.fetch_add(1, Ordering::Relaxed);
        Ok(block)
    }
    fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        assert!(
            riscv::register::sstatus::read().sie(),
            "allocation still masks IRQs"
        );
        let block = crate::metal::allocator::frame::instance().allocate_zeroed(layout)?;
        self.takes.fetch_add(1, Ordering::Relaxed);
        Ok(block)
    }
    unsafe fn deallocate(&self, pointer: NonNull<u8>, layout: Layout) {
        assert!(
            riscv::register::sstatus::read().sie(),
            "Page return still masks IRQs"
        );
        unsafe { crate::metal::allocator::frame::instance().deallocate(pointer, layout) };
        self.gives.fetch_add(1, Ordering::Relaxed);
    }
}
fn page(owner: Owner) -> Page {
    // SAFETY: Counted forwards the exact frame allocation/return receipts;
    // repartition therefore obeys that same production frame receipt contract.
    unsafe {
        Page::allocate(
            owner,
            level(),
            &COUNTED,
            crate::metal::allocator::frame::repartition,
        )
    }
    .unwrap()
}

struct Case {
    source: Arc<Flat>,
    target: Arc<Flat>,
    owner: Owner,
    prepared: AtomicBool,
    release: AtomicBool,
    done: AtomicUsize,
    owner_hart: AtomicUsize,
    peer_hart: AtomicUsize,
    ready: AtomicUsize,
    id: AtomicUsize,
    busy: AtomicUsize,
}
impl Case {
    fn new(owner: Owner) -> Arc<Self> {
        Arc::try_new(Self {
            source: flat(owner.clone(), 1),
            target: flat(Owner::Kernel, 1),
            owner,
            prepared: AtomicBool::new(false),
            release: AtomicBool::new(false),
            done: AtomicUsize::new(0),
            owner_hart: AtomicUsize::new(usize::MAX),
            peer_hart: AtomicUsize::new(usize::MAX),
            ready: AtomicUsize::new(0),
            id: AtomicUsize::new(0),
            busy: AtomicUsize::new(0),
        })
        .unwrap()
    }
}
static CURRENT: SpinLock<Option<Arc<Case>>> = SpinLock::new(None);
static WORKERS: [SpinLock<Option<Arc<Task>>>; 35] = [const { SpinLock::new(None) }; 35];
static NEXT: [AtomicUsize; 7] = [const { AtomicUsize::new(0) }; 7];

pub(super) fn prepare() -> Result<(), BootError> {
    // Build all stack/frame mappings before secondary Harts run. Concurrent
    // Kernel-Nest Task creation/retirement is a separate contract; do not make
    // its translations part of the Flat storage race being checked here.
    for (index, slot) in WORKERS.iter().enumerate() {
        let role = if index < 3 {
            index
        } else {
            3 + (index - 3) % 4
        };
        let task = kernel()
            .expect("kernel team")
            .task()
            .entry(VirtAddr::new(worker_entry(role)))
            .hold()
            .map_err(BootError::Bootstrap)?;
        *slot.lock() = Some(task);
    }
    Ok(())
}
fn worker_entry(role: usize) -> usize {
    // Match the existing Kernel observation Task: entry points take no args.
    // Exercise Flat, not the separate concurrent stack-argument writer.
    let entry = match role {
        0 => worker::<0> as *const (),
        1 => worker::<1> as *const (),
        2 => worker::<2> as *const (),
        3 => worker::<3> as *const (),
        4 => worker::<4> as *const (),
        5 => worker::<5> as *const (),
        6 => worker::<6> as *const (),
        _ => panic!("worker role"),
    };
    entry as usize
}
fn spawn(role: usize) -> TaskId {
    let round = NEXT[role].fetch_add(1, Ordering::Relaxed);
    let index = if role < 3 {
        assert_eq!(round, 0);
        role
    } else {
        assert!(round < 8);
        3 + round * 4 + role - 3
    };
    let task = WORKERS[index].lock().take().expect("prepared worker");
    let id = task.ident.id;
    Task::release(&task).unwrap();
    id
}
fn join(id: TaskId) {
    wait(|| {
        // Lookup's Hart-tracked weak references never cross a scheduling point.
        let _commit = crate::basis::commit();
        registry::find(id)
            .and_then(|weak| weak.upgrade())
            .is_none_or(|task| task.dead())
    });
}
fn install(case: &Arc<Case>) {
    assert!(CURRENT.lock().replace(case.clone()).is_none());
}
fn detach(case: &Arc<Case>) {
    let old = CURRENT.lock().take();
    drop(old);
    wait(|| Arc::strong_count(case) == 1);
}

extern "C" fn worker<const ROLE: usize>() -> ! {
    run_worker(ROLE)
}
fn run_worker(role: usize) -> ! {
    let case = CURRENT.lock().as_ref().unwrap().clone();
    match role {
        0 => {
            let reservation = reserve(&case.source);
            assert!(riscv::register::sstatus::read().sie());
            // Force a genuinely concurrent peer on another Hart. On one Hart,
            // use ordinary Task scheduling with the reservation still alive.
            let pin = (hart::hart_count() > 1).then(Pin::new);
            case.owner_hart
                .store(hart::hart_id().get(), Ordering::Relaxed);
            case.prepared.store(true, Ordering::Release);
            let deadline = until();
            while !case.release.load(Ordering::Acquire) {
                assert!(now() < deadline, "reservation peer stalled");
                if pin.is_some() {
                    crate::metal::interrupt::window::poll();
                    core::hint::spin_loop();
                } else {
                    yield_now();
                }
            }
            drop(pin);
            let cell = page(case.owner.clone());
            assert_eq!(cell.owner(), Owner::Kernel.identity());
            reservation.publish(cell, case.owner.identity()).unwrap();
            case.done.fetch_or(1, Ordering::Release);
        }
        1 => {
            wait(|| case.prepared.load(Ordering::Acquire));
            case.peer_hart
                .store(hart::hart_id().get(), Ordering::Relaxed);
            assert!(matches!(
                case.source.whole().materialize(),
                Err(FlatError::Busy)
            ));
            assert!(matches!(case.source.whole().reside(), Err(FlatError::Busy)));
            let outcome = Transfer::new(
                case.source.whole(),
                PageOffset::ZERO,
                case.target.whole(),
                PageOffset::ZERO,
            )
            .unwrap()
            .execute(None);
            assert_eq!(outcome.result, Err(TransferError::Busy));
            case.owner.close();
            case.release.store(true, Ordering::Release);
            case.done.fetch_or(2, Ordering::Release);
        }
        2 => {
            let result = (|| -> Result<(), FlatError> {
                let _reservation = reserve(&case.source);
                let cell = page(Owner::Kernel);
                case.id.store(cell.id().get(), Ordering::Relaxed);
                let _unpublished = cell;
                Err(FlatError::Busy) // cancel after physical allocation
            })();
            assert_eq!(result, Err(FlatError::Busy));
            case.done.store(1, Ordering::Release);
        }
        3..=6 => {
            case.ready.fetch_add(1, Ordering::Release);
            wait(|| case.ready.load(Ordering::Acquire) == 4);
            let deadline = until();
            loop {
                match case.source.whole().materialize() {
                    Ok(pages) => {
                        let id = resident(&case.source, 0).0;
                        if let Err(old) =
                            case.id
                                .compare_exchange(0, id, Ordering::AcqRel, Ordering::Acquire)
                        {
                            assert_eq!(old, id, "two Pages at the same position");
                        }
                        drop(pages);
                        break;
                    }
                    Err(FlatError::Busy) => {
                        case.busy.fetch_add(1, Ordering::Relaxed);
                        assert!(now() < deadline, "materialization stayed Busy");
                        yield_now();
                    }
                    Err(error) => panic!("concurrent materialization: {error:?}"),
                }
            }
            case.done.fetch_add(1, Ordering::Release);
        }
        _ => panic!("worker role"),
    }
    drop(case);
    exit()
}

fn transfer(case: &Case) {
    let before = resident(&case.source, 0);
    unsafe { (before.1 as *mut u64).write_volatile(COOKIE) };
    let (transaction, ()) = super::super::transaction::Transaction::claim(
        &case.source.whole(),
        PageIndex::ZERO,
        &case.target.whole(),
        PageIndex::ZERO,
        None,
        || Ok(()),
    )
    .unwrap();
    assert!(matches!(
        case.source.whole().materialize(),
        Err(FlatError::Busy)
    ));
    assert!(matches!(
        case.target.whole().materialize(),
        Err(FlatError::Busy)
    ));
    drop(transaction);
    assert_eq!(resident(&case.source, 0), before);
    assert!(case.target.can_materialize(PageIndex::ZERO).unwrap());
    let outcome = Transfer::new(
        case.source.whole(),
        PageOffset::ZERO,
        case.target.whole(),
        PageOffset::ZERO,
    )
    .unwrap()
    .execute(None);
    assert_eq!(outcome.result, Ok(()));
    assert_eq!(resident(&case.target, 0), before);
    assert!(matches!(
        case.source.whole().materialize(),
        Err(FlatError::Missing)
    ));
    let (transaction, ()) = super::super::transaction::Transaction::claim(
        &case.target.whole(),
        PageIndex::ZERO,
        &case.source.whole(),
        PageIndex::ZERO,
        None,
        || Ok(()),
    )
    .unwrap();
    drop(transaction); // restore the old Missing destination, never Fresh
    assert!(matches!(
        case.source.whole().materialize(),
        Err(FlatError::Missing)
    ));
    assert_eq!(unsafe { (before.1 as *const u64).read_volatile() }, COOKIE);
}

fn allocation_failure() {
    // Run before creating peers; no concurrent Task can change allocator setup.
    let _pin = Pin::new();
    let source = flat(Owner::Kernel, 2);
    {
        let mut state = source.state.lock();
        state.residencies.try_reserve(2).unwrap();
        let FlatSource::Managed(slots) = &mut state.source else {
            unreachable!()
        };
        slots.try_reserve_exact(2).unwrap();
    }
    // Keep an Arc<Page> size-class pool live with spare slots. The one released
    // 2 MiB block must go to data, not a first-use metadata pool.
    let mut warm = Vec::new();
    for _ in 0..16 {
        warm.push(Arc::try_new(cell::allocate(Owner::Kernel).unwrap()).unwrap());
    }
    let _anchor = warm.pop().unwrap();
    drop(warm);
    let mut held = Vec::new();
    held.try_reserve_exact(64).unwrap();
    loop {
        match cell::allocate_level(Owner::Kernel, level()) {
            Ok(page) => {
                assert!(held.len() < 64, "fixture RAM exceeds bound");
                held.push(page);
            }
            Err(FlatError::OutOfMemory) => break,
            Err(error) => panic!("exhaustion setup: {error:?}"),
        }
    }
    drop(held.pop().expect("at least one 2 MiB allocation"));
    assert!(matches!(
        source.whole().materialize(),
        Err(FlatError::OutOfMemory)
    ));
    let prefix = resident(&source, 0);
    {
        let state = source.state.lock();
        let FlatSource::Managed(slots) = &state.source else {
            unreachable!()
        };
        assert_eq!(slots.len(), 1, "failed reservation was not cancelled");
    }
    unsafe { (prefix.1 as *mut u64).write_volatile(COOKIE) };
    assert!(matches!(
        source.whole().materialize(),
        Err(FlatError::OutOfMemory)
    ));
    assert_eq!(resident(&source, 0), prefix);
    drop(held);
    let pages = source.whole().materialize().unwrap();
    assert_eq!(resident(&source, 0), prefix);
    assert_eq!(unsafe { (prefix.1 as *const u64).read_volatile() }, COOKIE);
    let next = resident(&source, GRAIN);
    assert_ne!(next.0, prefix.0);
    assert_eq!(unsafe { (next.1 as *const u64).read_volatile() }, 0);
    assert_eq!(
        unsafe { ((next.1 + 2 * 1024 * 1024 - 8) as *const u64).read_volatile() },
        0
    );
    drop(pages);
}

pub(super) extern "C" fn run() -> ! {
    wait(|| (0..hart::hart_count()).all(|at| hart::online(hart::HartId::new(at))));
    allocation_failure();
    note(format_args!(
        "PASS flat-oom actual-exhaustion=1 prefix=preserved retry=resident"
    ));

    let case = Case::new(Owner::team(TeamId::new(103)).unwrap());
    install(&case);
    let returns = COUNTED.gives.load(Ordering::Relaxed);
    let a = spawn(0);
    let b = spawn(1);
    wait(|| case.done.load(Ordering::Acquire) == 3);
    join(a);
    join(b);
    if hart::hart_count() > 1 {
        assert_ne!(
            case.owner_hart.load(Ordering::Relaxed),
            case.peer_hart.load(Ordering::Relaxed)
        );
    }
    transfer(&case);
    let pair = (
        case.owner_hart.load(Ordering::Relaxed),
        case.peer_hart.load(Ordering::Relaxed),
    );
    detach(&case);
    drop(case);
    assert_eq!(COUNTED.gives.load(Ordering::Relaxed), returns + 1);
    note(format_args!(
        "PASS flat-reservation owner-hart={} peer-hart={} busy=consistent owner-close=kernel transfer=identity",
        pair.0, pair.1
    ));

    let case = Case::new(Owner::Kernel);
    install(&case);
    let returns = COUNTED.gives.load(Ordering::Relaxed);
    let task = spawn(2);
    wait(|| case.done.load(Ordering::Acquire) == 1);
    join(task);
    assert_eq!(COUNTED.gives.load(Ordering::Relaxed), returns + 1);
    let pages = case.source.whole().materialize().unwrap();
    assert_ne!(resident(&case.source, 0).0, case.id.load(Ordering::Relaxed));
    drop(pages);
    detach(&case);
    drop(case);
    note(format_args!(
        "PASS flat-cancel unpublished-return=once task-exit=observed retry=fresh"
    ));

    let mut busy = 0;
    for _ in 0..8 {
        let case = Case::new(Owner::Kernel);
        install(&case);
        let tasks = [spawn(3), spawn(4), spawn(5), spawn(6)];
        wait(|| case.done.load(Ordering::Acquire) == 4);
        for task in tasks {
            join(task);
        }
        busy += case.busy.load(Ordering::Relaxed);
        assert_eq!(resident(&case.source, 0).0, case.id.load(Ordering::Relaxed));
        transfer(&case);
        detach(&case);
        drop(case);
    }
    note(format_args!(
        "PASS flat-race rounds=8 workers=4 busy={} unique-page=1 missing=sticky",
        busy
    ));
    assert_eq!(
        COUNTED.takes.load(Ordering::Relaxed),
        COUNTED.gives.load(Ordering::Relaxed)
    );
    note(format_args!(
        "PASS flat-contracts harts={} abi={} counted-returns={}",
        hart::hart_count(),
        kernel_api::abi::VERSION,
        COUNTED.gives.load(Ordering::Relaxed)
    ));
    exit()
}
