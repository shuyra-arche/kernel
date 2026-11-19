//! Page policy runs in the caller; a separate Task only saves/restores bytes.
//! Backing is a bounded in-memory fixture, not a claim of durable disk storage.
use super::{fault, support::*};
use core::{
    fmt::Write,
    sync::atomic::{AtomicUsize, Ordering},
};
use kernel_api::{
    abi::memory::*,
    address::VirtAddr,
    model::{cell::Placement, flat::SlotPhase, nest::FaultCause, unit::*, wait::Wait},
};
use kernel_client::call::{room, unit};
use kernel_client::memory::Response;
#[derive(Clone, Copy)]
#[repr(usize)]
enum Command {
    Idle = 0,
    Save = 1,
    Restore = 2,
    Stop = 3,
}
static REQUEST: AtomicUsize = AtomicUsize::new(Command::Idle as usize);
static ACK: AtomicUsize = AtomicUsize::new(Command::Idle as usize);
static BUFFER: AtomicUsize = AtomicUsize::new(0);
static APPLICATION: AtomicUsize = AtomicUsize::new(0);
const CONTENT: u64 = 0x917435aca7b83e22;
extern "C" fn storage() -> ! {
    let mut backing = [0u64; 512];
    for operation in [Command::Save, Command::Restore, Command::Stop] {
        let end = room::clock() + 5_000_000_000;
        while REQUEST.load(Ordering::Acquire) != operation as usize {
            assert!(room::clock() < end);
            room::yield_();
        }
        let address = BUFFER.load(Ordering::Relaxed);
        match operation {
            Command::Save => {
                for (i, value) in backing.iter_mut().enumerate() {
                    *value = unsafe { ((address + i * 8) as *const u64).read_volatile() };
                    assert_eq!(*value, CONTENT ^ (i as u64));
                }
            }
            Command::Restore => {
                for (i, value) in backing.iter().enumerate() {
                    unsafe { ((address + i * 8) as *mut u64).write_volatile(*value) };
                }
            }
            Command::Stop => {}
            Command::Idle => unreachable!("storage worker has no idle operation"),
        }
        ACK.store(operation as usize, Ordering::Release);
    }
    unit::exit(74, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
fn storage_request(operation: Command, address: VirtAddr) {
    BUFFER.store(address.get(), Ordering::Relaxed);
    REQUEST.store(operation as usize, Ordering::Release);
    let end = room::clock() + 5_000_000_000;
    while ACK.load(Ordering::Acquire) != operation as usize {
        assert!(room::clock() < end);
        room::yield_();
    }
}
extern "C" fn blocked_writer() -> ! {
    unsafe { (APPLICATION.load(Ordering::Acquire) as *mut u64).write_volatile(17) };
    unit::exit(99, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
pub fn run() {
    let worker = spawn(storage);
    unit::resume(UnitTarget::Task(worker)).unwrap();
    let slot = create(1);
    mutate(Action::Occupy {
        slot,
        expected: slot_status(slot).version,
    });
    let (app, app_va) = reserve(space());
    insert(app, slot);
    publish(app);
    for i in 0..512 {
        unsafe { ((app_va.get() + i * 8) as *mut u64).write_volatile(CONTENT ^ (i as u64)) };
    }
    let (service, service_va) = reserve_at(space(), Placement::lowest(4096).unwrap(), 2);
    insert(service, slot);
    let maintenance = block(slot);
    APPLICATION.store(app_va.get(), Ordering::Release);
    let writer = spawn(blocked_writer);
    unit::resume(UnitTarget::Task(writer)).unwrap();
    assert_eq!(
        join(writer).exit,
        Some((ExitCause::Fault, Failure::Busy as i64 as u64))
    );
    mutate(Action::Publish {
        memo: service,
        expected: memo_status(service).generation,
        maintenance: Some(maintenance),
    });
    storage_request(Command::Save, service_va);
    // Maintenance access is a real responsibility: release must refuse until retired.
    let Response::Reply(reply) = call(Action::Vacate {
        slot,
        expected: slot_status(slot).version,
        maintenance,
    }) else {
        panic!("Vacate")
    };
    assert!(matches!(reply.outcome(), Outcome::Rejected(Failure::Busy)));
    retract(service);
    mutate(Action::Vacate {
        slot,
        expected: slot_status(slot).version,
        maintenance,
    });
    mutate(Action::Allow { maintenance });
    assert_eq!(slot_status(slot).phase, SlotPhase::Missing);
    let Response::Reply(reply) = call(Action::Occupy {
        slot,
        expected: slot_status(slot).version,
    }) else {
        panic!("Occupy")
    };
    assert!(!matches!(reply.outcome(), Outcome::Committed(_)));
    assert_eq!(slot_status(slot).phase, SlotPhase::Missing);
    let (reader, epoch) = fault::start_reader(app, app_va, CONTENT);
    let event = fault::event(app, epoch, Wait::At(room::clock() + 2_000_000_000));
    assert_eq!(event.cause, FaultCause::Missing);
    assert_eq!(event.flat, slot.flat());
    let staging = create(1);
    mutate(Action::Occupy {
        slot: staging,
        expected: slot_status(staging).version,
    });
    let (scratch, scratch_va) = reserve(space());
    insert(scratch, staging);
    publish(scratch);
    storage_request(Command::Restore, scratch_va);
    retract(scratch);
    let maintenance = block(slot);
    mutate(Action::Push {
        local: staging,
        local_version: slot_status(staging).version,
        remote: slot,
        remote_version: slot_status(slot).version,
        maintenance: Some(maintenance),
    });
    mutate(Action::Allow { maintenance });
    assert_ne!(
        task_info(UnitQuery::Target(UnitTarget::Task(reader))).state,
        UnitState::Exited
    );
    publish(app);
    assert_eq!(join(reader).result, Some(42));
    storage_request(Command::Stop, VirtAddr::new(0));
    assert_eq!(join(worker).result, Some(74));
    let _ = writeln!(
        Console,
        "PASS native-swap stable-copy=1 blocked-writer=1 maintenance-retention=1 missing-not-zero=1 logical-source-retained=1 restored-bytes=4096 passive-storage=1"
    );
}
