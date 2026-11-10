use kernel_api::{
    abi::reply::SignReply,
    address::VirtAddr,
    boot::{
        args,
        ledger::{Entries, Name},
    },
    model::{
        identity::{Mark, PendId},
        memory::{Snapshot, SnapshotPage, UNKNOWN},
        modifiable::Modifiable,
        pend::{
            AccordRequest, AccordTarget, Distributable, Limits, Operations, PendRange, ShareRights,
        },
        wait::Wait,
    },
};
use kernel_client::call::{pend, room, share_view, sign};
fn next(page: &SnapshotPage, epoch: u64) -> Snapshot {
    let end = room::clock() + 2_000_000_000;
    loop {
        if let Some(value) = page.read() {
            if value.epoch > epoch {
                return value;
            }
        }
        assert!(room::clock() < end, "observer sample stalled");
        room::yield_();
    }
}
pub(super) fn run(at: usize, count: usize) {
    let words = unsafe { core::slice::from_raw_parts(at as *const usize, count) };
    let bytes = unsafe {
        core::slice::from_raw_parts(words[args::LEDGER] as *const u8, words[args::LEDGER_LEN])
    };
    let ((_, view), notification) = Entries::new(bytes)
        .unwrap()
        .find(Name::Memory)
        .unwrap()
        .dispatch()
        .unwrap();
    let view = PendId::mint(view);
    let notification = PendId::mint(notification);
    assert!(sign::trigger(notification).is_err());
    let rights = Limits {
        operations: Operations::Share(ShareRights::OPEN | ShareRights::SHUT),
        modifiable: Modifiable::No,
        distributable: Distributable::No,
        range: PendRange::Inherit,
    };
    let request = |modifiable| {
        AccordRequest {
            src: view,
            target: AccordTarget::Caller,
            limits: Limits {
                modifiable,
                ..rights
            },
            mark: Mark::NONE,
        }
        .to_bytes()
    };
    assert!(pend::accord(crate::ptr(&request(Modifiable::Yes))).is_err());
    let child = pend::accord(crate::ptr(&request(Modifiable::No))).unwrap();
    let (address, len) = share_view::open(child).unwrap();
    assert_eq!(len, 4096);
    let page = unsafe { &*(address.get() as *const SnapshotPage) };
    let before = next(page, 0);
    assert!(before.capacity > 0);
    assert_eq!(before.capacity, before.available + before.resident);
    assert_eq!((before.accessed, before.dirty), (UNKNOWN, UNKNOWN));
    assert!(before.start_ns <= before.end_ns && before.end_ns <= room::clock());
    use kernel_api::model::cell::{Allocation, CellCreate, CellLevel, CreatedCell};
    let allocation =
        CellCreate::Slot(Allocation::at_level(CellLevel::new(1).unwrap(), 1).unwrap()).to_bytes();
    let CreatedCell::Slot(backing) =
        kernel_client::call::cell::create(crate::license::get(), crate::ptr(&allocation)).unwrap()
    else {
        panic!("Slot expected");
    };
    let (mapping, at) = crate::reserve(backing, 512);
    crate::handoff(crate::slot(backing, 0), crate::memo(mapping, 0), false);
    crate::publish(mapping, 0);
    crate::write(at, 0, 0x137);
    let finished = room::clock();
    let mut after = next(page, before.epoch);
    while after.start_ns < finished {
        after = next(page, after.epoch);
    }
    assert_eq!(after.capacity, after.available + after.resident);
    // Other Harts can release boot heap/table storage during this interval.
    // The level-1 Cell must contribute bytes (about 2 MiB), never one event.
    assert!(
        after.resident > before.resident + 1024 * 1024,
        "before={before:?} after={after:?}"
    );
    let reply = sign::listen(notification, Wait::At(room::clock() + 2_000_000_000)).unwrap();
    let SignReply::Triggered { sequence } = reply else {
        panic!("observer notification missing");
    };
    sign::consume(notification, sequence).unwrap();
    crate::retract(mapping, 0);
    crate::delete(crate::raw_memo(mapping, 0).object);
    crate::delete(kernel_api::model::cell::CellObject::Slot(backing));
    let finished = room::clock();
    let mut later = next(page, after.epoch);
    while later.start_ns < finished {
        later = next(page, later.epoch);
    }
    assert!(later.available > after.available);
    // Revoke closes the mapped child; no raw access is attempted after revocation.
    crate::lifecycle::return_pend(child);
    assert!(share_view::open(child).is_err());
    let (address, _) = share_view::open(view).unwrap();
    let page = unsafe { &*(address.get() as *const SnapshotPage) };
    let still_live = next(page, later.epoch);
    assert_eq!(
        still_live.capacity,
        still_live.available + still_live.resident
    );
    share_view::shut(view).unwrap();
    use core::fmt::Write;
    let _ = writeln!(
        crate::Console,
        "PASS memory-observer task=1 snapshot=1 bytes=1 readonly=1 revoke=1 sign=1 unknown-ad=1"
    );
    let _ = VirtAddr::new(0);
}
