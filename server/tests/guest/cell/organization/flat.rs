use crate::{allocate, attach, delete, handoff, ptr, raw_memo, read, retract, slot, write};
use kernel_api::{
    address::VirtAddr,
    model::{
        cell::*,
        flat::FlatInfo,
        identity::{Mark, PendId},
        modifiable::Modifiable,
        pend::{
            AccordRequest, AccordTarget, Distributable, FlatRights, Limits, NarrowRequest,
            Operations, PendRange,
        },
    },
};
use kernel_client::call::{cell, flat, pend, share_view};
fn inspect(token: PendId) -> FlatInfo {
    let mut bytes = [0; FlatInfo::WIDTH];
    flat::inspect(token, VirtAddr::new(bytes.as_mut_ptr() as usize)).unwrap();
    FlatInfo::from_bytes(&bytes).unwrap()
}
fn derive(root: PendId, rights: FlatRights, offset: usize, count: usize) -> PendId {
    let request = AccordRequest {
        src: root,
        target: AccordTarget::Caller,
        limits: Limits {
            operations: Operations::Flat(rights),
            modifiable: Modifiable::Yes,
            distributable: Distributable::No,
            range: PendRange::Pages { offset, count },
        },
        mark: Mark::NONE,
    }
    .to_bytes();
    pend::accord(ptr(&request)).unwrap()
}
fn accessible(reference: SlotId) -> bool {
    let mut bytes = [0; CellInfo::WIDTH];
    cell::inspect(
        ptr(&slot(reference, 0).to_bytes()),
        VirtAddr::new(bytes.as_mut_ptr() as usize),
    )
    .is_ok()
}
pub(super) fn run() {
    assert!(flat::create(PendId::NONE, 4096, 4096).is_err());
    let root = flat::create(crate::license::get(), 8192, 4096).unwrap();
    assert_eq!(inspect(root).count, 2);
    let identity = inspect(root).id;
    let original = flat::slice(root, 0, 2).unwrap();
    assert!(flat::slice(root, 2, 1).is_err());
    let child = derive(
        root,
        FlatRights::INSPECT | FlatRights::ASSOCIATE | FlatRights::TRANSFER,
        1,
        1,
    );
    assert_eq!(inspect(child).id, identity);
    assert_eq!(inspect(child).offset, 1);
    assert!(!accessible(original)); // mutable loan suspends the parent's old Slot
    let sliced = flat::slice(child, 0, 1).unwrap();
    assert!(flat::slice(child, 1, 1).is_err());
    assert!(share_view::create(crate::license::get(), sliced, Modifiable::No).is_err());
    let (mapping, address) = attach(sliced, 1);
    write(address, 0, 0x8354);
    let donor = allocate(1);
    let (staging, temporary) = attach(donor, 1);
    write(temporary, 0, 0x84);
    retract(mapping, 0);
    retract(staging, 0);
    delete(raw_memo(staging, 0).object);
    let receiver = allocate(1);
    // Transfer remains a separate right; the exact delegated branch supplies it.
    handoff(crate::memo(mapping, 0), slot(receiver, 0), false);
    handoff(slot(donor, 0), slot(sliced, 0), false);
    delete(raw_memo(mapping, 0).object);
    pend::revoke(crate::lifecycle::self_id(), child).unwrap();
    assert!(!accessible(sliced));
    assert!(accessible(original));
    delete(CellObject::Slot(sliced));
    delete(CellObject::Slot(donor));
    delete(CellObject::Slot(receiver));
    // Flat -> Share is one child in the same revoke tree, with the same Page.
    let one = flat::slice(root, 1, 1).unwrap();
    let share = share_view::create(crate::license::get(), one, Modifiable::No).unwrap();
    let (address, size) = share_view::open(share).unwrap();
    assert_eq!(size, 4096);
    assert_eq!(read(address, 0), 0x84);
    assert!(!accessible(one));
    pend::return_(share).unwrap();
    assert!(accessible(one));
    // Existing handles cannot retain a window or operation after Narrow.
    let request = NarrowRequest {
        token: root,
        limits: Limits {
            operations: Operations::Flat(
                FlatRights::INSPECT | FlatRights::ASSOCIATE | FlatRights::DELETE,
            ),
            modifiable: Modifiable::Yes,
            distributable: Distributable::No,
            range: PendRange::Pages {
                offset: 1,
                count: 1,
            },
        },
    }
    .to_bytes();
    pend::narrow(ptr(&request)).unwrap();
    assert!(!accessible(original));
    assert!(accessible(one));
    let destination = allocate(1);
    assert!(
        cell::push(ptr(&CellHandoff {
            source: slot(one, 0),
            target: slot(destination, 0)
        }
        .to_bytes()))
        .is_err()
    );
    flat::delete(root).unwrap();
    assert!(!accessible(one));
    pend::return_(root).unwrap();
    delete(CellObject::Slot(destination));
    delete(CellObject::Slot(original));
    delete(CellObject::Slot(one));
}
