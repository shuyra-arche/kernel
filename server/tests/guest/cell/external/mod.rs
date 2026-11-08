use core::fmt::Write;
use kernel_api::{
    address::VirtAddr,
    boot::{
        args,
        ledger::{Entries, Name, Page},
    },
    model::{cell::*, identity::PendId},
};
use kernel_client::call::{cell, room, share_view};
fn info(pos: CellPosition) -> CellInfo {
    super::info(pos)
}
fn current(mut pos: CellPosition) -> CellPosition {
    pos.generation = info(pos).generation;
    pos
}
fn write(base: VirtAddr, offset: usize, value: u64) {
    unsafe {
        core::arch::asm!("fence iorw, iorw", options(nostack));
        ((base.get() + offset) as *mut u64).write_volatile(value);
        core::arch::asm!("fence iorw, iorw", options(nostack));
    }
}
fn read(base: VirtAddr, offset: usize) -> u32 {
    unsafe {
        core::arch::asm!("fence iorw, iorw", options(nostack));
        ((base.get() + offset) as *const u32).read_volatile()
    }
}
pub(super) fn run(at: usize, count: usize) {
    assert_eq!(count, args::LEN);
    let words = unsafe { core::slice::from_raw_parts(at as *const usize, count) };
    let ledger = Entries::new(unsafe {
        core::slice::from_raw_parts(words[args::LEDGER] as *const u8, words[args::LEDGER_LEN])
    })
    .unwrap();
    let entry = ledger
        .find(Name::Page(Page::Region(0x4000_0000)))
        .expect("trusted EDU BAR");
    let token = PendId::mint(entry.handle());
    let (bar, _) = share_view::open(token).unwrap();
    assert_eq!(read(bar, 0) & 0xffff, 0xed);
    let slot = super::allocate(1);
    let empty = super::allocate(1);
    let (cpu, address) = super::attach(slot, 1);
    super::write(address, 0, 0x5a5a5a5a);
    super::retract(cpu, 0);
    let spec = CellCreate::Memo {
        source: CellSource::Slot(ReferenceRange {
            reference: slot,
            offset: 0,
            count: 1,
        }),
        destination: MemoDestination::External {
            authority: token,
            address: VirtAddr::new(0x400000),
            access: kernel_api::model::modifiable::Modifiable::No,
        },
    }
    .to_bytes();
    let made = cell::create(kernel_api::model::identity::PendId::NONE, super::ptr(&spec)).unwrap();
    let CreatedCell::Memo {
        mapping,
        address: _address,
    } = made
    else {
        panic!("expected Memo")
    };
    let pos = CellPosition {
        object: CellObject::Memo {
            target: MappingTarget::Io(token),
            mapping,
        },
        page: 0,
        generation: 0,
    };
    super::handoff(super::slot(slot, 0), current(pos), false);
    assert_eq!(info(pos).state, CellState::Ready);
    cell::publish(super::ptr(&current(pos).to_bytes())).unwrap();
    let until = room::clock() + 2_000_000_000;
    while info(pos).state != CellState::Published {
        assert!(room::clock() < until, "IOMMU publish deadline");
        room::yield_();
    }
    assert!(info(pos).external);
    assert!(info(pos).frozen); // immutable DMA does not reopen writers
    let move_page = CellHandoff {
        source: super::slot(slot, 0),
        target: super::slot(empty, 0),
    }
    .to_bytes();
    assert!(cell::push(super::ptr(&move_page)).is_err());
    assert!(cell::publish(super::ptr(&super::memo(cpu, 0).to_bytes())).is_err());
    // An actual device write through a read-only IOVA must fault.
    write(bar, 0x80, 0x40000);
    write(bar, 0x88, 0x400000);
    write(bar, 0x90, 64);
    write(bar, 0x98, 3);
    let until = room::clock() + 2_000_000_000;
    while read(bar, 0x98) & 1 != 0 {
        assert!(room::clock() < until, "EDU DMA deadline");
        room::sleep_until(room::clock() + 1_000_000).unwrap();
    }
    while info(pos).fault_cause == 0 {
        assert!(room::clock() < until, "IOMMU fault deadline");
        room::yield_();
    }
    assert!(info(pos).external); // business completion is not retirement
    cell::retract(super::ptr(&current(pos).to_bytes())).unwrap();
    let until = room::clock() + 2_000_000_000;
    while info(pos).state != CellState::Ready {
        assert!(room::clock() < until, "IOMMU retirement deadline");
        room::yield_();
    }
    assert!(!info(pos).external);
    assert!(info(pos).frozen);
    assert!(cell::publish(super::ptr(&current(pos).to_bytes())).is_err()); // one-shot platform route
    super::handoff(current(pos), super::slot(empty, 0), true);
    assert_eq!(info(pos).page, info(super::slot(empty, 0)).page);
    super::handoff(super::slot(empty, 0), super::slot(slot, 0), false);
    super::delete(pos.object);
    super::publish(cpu, 0);
    assert_eq!(super::read(address, 0), 0x5a5a5a5a);
    super::retract(cpu, 0);
    super::delete(super::raw_memo(cpu, 0).object);
    super::delete(CellObject::Slot(slot));
    super::delete(CellObject::Slot(empty));
    share_view::shut(token).unwrap();
    let _ = writeln!(
        super::Console,
        "PASS cell-io external=1 readonly=1 fault=1 fenced=1 ownership=1"
    );
}

use core::sync::atomic::{AtomicUsize, Ordering};
static BORROWED_BAR: AtomicUsize = AtomicUsize::new(0);
extern "C" fn publisher() -> ! {
    use kernel_client::call::unit;
    let token = PendId::mint(BORROWED_BAR.load(Ordering::Acquire));
    let (bar, _) = share_view::open(token).unwrap();
    let slot = super::allocate(1);
    let spec = CellCreate::Memo {
        source: CellSource::Slot(ReferenceRange {
            reference: slot,
            offset: 0,
            count: 1,
        }),
        destination: MemoDestination::External {
            authority: token,
            address: VirtAddr::new(0x400000),
            access: kernel_api::model::modifiable::Modifiable::No,
        },
    }
    .to_bytes();
    let made = cell::create(kernel_api::model::identity::PendId::NONE, super::ptr(&spec)).unwrap();
    let CreatedCell::Memo {
        mapping,
        address: _address,
    } = made
    else {
        panic!("expected Memo")
    };
    let pos = CellPosition {
        object: CellObject::Memo {
            target: MappingTarget::Io(token),
            mapping,
        },
        page: 0,
        generation: 1,
    };
    super::handoff(super::slot(slot, 0), pos, false);
    cell::publish(super::ptr(&current(pos).to_bytes())).unwrap();
    let until = room::clock() + 2_000_000_000;
    while info(pos).state != CellState::Published {
        assert!(room::clock() < until);
        room::yield_();
    }
    // EDU schedules the DMA callback later. Exit cannot discharge that device obligation.
    write(bar, 0x80, 0x400000);
    write(bar, 0x88, 0x40000);
    write(bar, 0x90, 64);
    write(bar, 0x98, 1);
    unit::exit(77, 0, VirtAddr::new(0), 0)
}
pub(super) fn death(at: usize, count: usize) {
    use kernel_api::model::{
        identity::Mark,
        modifiable::Modifiable,
        pend::{AccordRequest, AccordTarget, PendInfo, PendRange, PendStatus},
        unit::UnitTarget,
    };
    use kernel_client::call::{pend, unit};
    assert_eq!(count, args::LEN);
    let words = unsafe { core::slice::from_raw_parts(at as *const usize, count) };
    let ledger = Entries::new(unsafe {
        core::slice::from_raw_parts(words[args::LEDGER] as *const u8, words[args::LEDGER_LEN])
    })
    .unwrap();
    let root = PendId::mint(
        ledger
            .find(Name::Page(Page::Region(0x4000_0000)))
            .unwrap()
            .handle(),
    );
    assert!(share_view::delete(root).is_err());
    let task = super::lifecycle::spawn(publisher as *const () as usize);
    crate::license::delegate(task);
    let request = AccordRequest {
        src: root,
        target: AccordTarget::Task(task),
        limits: kernel_api::model::pend::Limits {
            operations: kernel_api::model::pend::Operations::Share(
                kernel_api::model::pend::ShareRights::OPEN
                    | kernel_api::model::pend::ShareRights::SHUT
                    | kernel_api::model::pend::ShareRights::ASSOCIATE,
            ),
            modifiable: Modifiable::Yes,
            distributable: kernel_api::model::pend::Distributable::Yes,
            range: PendRange::Inherit,
        },
        mark: Mark::NONE,
    }
    .to_bytes();
    let token = pend::accord(super::ptr(&request)).unwrap();
    BORROWED_BAR.store(token.get(), Ordering::Release);
    assert!(share_view::open(root).is_err());
    unit::resume(UnitTarget::Task(task)).unwrap();
    assert!(super::lifecycle::wait_task(task));
    assert_eq!(super::lifecycle::result(task), (1, 77));
    let until = room::clock() + 5_000_000_000;
    loop {
        let mut bytes = [0; PendInfo::SIZE];
        pend::inspect(root, VirtAddr::new(bytes.as_mut_ptr() as usize)).unwrap();
        if PendInfo::from_bytes(&bytes).unwrap().status == PendStatus::Active {
            break;
        }
        assert!(
            room::clock() < until,
            "dead publisher retained target forever"
        );
        room::yield_();
    }
    // Restored authority does not re-enable the retired one-shot PCI route.
    let _ = share_view::open(root).unwrap();
    share_view::shut(root).unwrap();
    super::lifecycle::delete(task);
    let _ = writeln!(super::Console, "PASS cell-io death=1 result=1 retirement=1");
}
