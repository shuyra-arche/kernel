use crate::{
    Console, delete, handoff, info, memo, ptr, publish, raw_memo, read, retract, slot, write,
};
use core::fmt::Write;
use kernel_api::{
    address::VirtAddr,
    model::{
        cell::*,
        identity::{PendId, TeamId},
    },
};
use kernel_client::call::cell;
mod authority;
pub(crate) fn allocate(level: usize, count: usize) -> SlotId {
    let request =
        CellCreate::Slot(Allocation::at_level(CellLevel::new(level).unwrap(), count).unwrap())
            .to_bytes();
    let CreatedCell::Slot(slot) = cell::create(crate::license::get(), ptr(&request)).unwrap()
    else {
        panic!("Slot expected")
    };
    slot
}
pub(crate) fn attach_large(slot_id: SlotId) -> (kernel_api::model::identity::PendId, VirtAddr) {
    let (mapping, address) = crate::reserve(slot_id, 512);
    handoff(slot(slot_id, 0), memo(mapping, 0), false);
    assert_eq!(info(raw_memo(mapping, 0)).level.get(), 1);
    publish(mapping, 0);
    (mapping, address)
}
fn absent(slot_id: SlotId) {
    let mut out = [0; CellInfo::WIDTH];
    assert!(
        cell::inspect(
            ptr(&slot(slot_id, 0).to_bytes()),
            VirtAddr::new(out.as_mut_ptr() as usize)
        )
        .is_err()
    );
}
fn reserve_huge(reference: SlotId) -> (kernel_api::model::identity::PendId, VirtAddr) {
    let request = CellCreate::Memo {
        source: CellSource::Slot(ReferenceRange {
            reference,
            offset: 0,
            count: 1 << 18,
        }),
        destination: MemoDestination::Internal {
            team: TeamId::new(0),
            placement: Placement::lowest(1 << 30).unwrap(),
            access: 6,
        },
    }
    .to_bytes();
    let CreatedCell::Memo {
        token: mapping,
        address,
    } = cell::create(PendId::NONE, ptr(&request)).unwrap()
    else {
        panic!("Memo expected")
    };
    (mapping, address)
}
pub(crate) fn run() {
    authority::run();
    let a = allocate(1, 1);
    let b = allocate(1, 1);
    assert_eq!(info(slot(a, 0)).level.get(), 1);
    let (mapping, address) = attach_large(a);
    let old_id = info(slot(a, 0)).page;
    let offsets = [0, 4096, 1 << 20, (1 << 21) - 8];
    for offset in offsets {
        assert_eq!(read(address, offset), 0);
        write(address, offset, 0x12340000 + offset);
    }
    assert!(cell::split(a).is_err());
    let mut out = [0; CellInfo::WIDTH];
    assert!(
        cell::inspect(
            ptr(&slot(a, 1).to_bytes()),
            VirtAddr::new(out.as_mut_ptr() as usize)
        )
        .is_err()
    );
    let partial = CellCreate::Memo {
        source: CellSource::Slot(ReferenceRange {
            reference: a,
            offset: 0,
            count: 1,
        }),
        destination: MemoDestination::Internal {
            team: TeamId::new(0),
            placement: Placement::lowest(4096).unwrap(),
            access: 6,
        },
    }
    .to_bytes();
    assert!(cell::create(PendId::NONE, ptr(&partial)).is_err());
    handoff(slot(a, 0), slot(b, 0), false);
    assert_eq!(info(slot(b, 0)).page, old_id);
    assert!(cell::split(b).is_err());
    for offset in offsets {
        assert_eq!(read(address, offset), 0x12340000 + offset);
    }
    retract(mapping, 0);
    delete(raw_memo(mapping, 0).object);
    let children = cell::split(b).unwrap();
    absent(b);
    assert_eq!(info(slot(children, 0)).level.get(), 0);
    assert_ne!(info(slot(children, 0)).page, old_id);
    assert_ne!(info(slot(children, 511)).page, old_id);
    let (small, at) = crate::reserve(children, 512);
    for n in [0, 511] {
        handoff(slot(children, n), memo(small, n), false);
        publish(small, n);
    }
    assert_eq!(read(at, 0), 0x12340000);
    assert_eq!(read(at, (1 << 21) - 8), 0x12340000 + (1 << 21) - 8);
    assert!(cell::merge(children).is_err());
    for n in [0, 511] {
        retract(small, n);
    }
    delete(raw_memo(small, 0).object);
    let merged = cell::merge(children).unwrap();
    absent(children);
    let (mapping, address) = attach_large(merged);
    assert_ne!(info(slot(merged, 0)).page, old_id);
    for offset in offsets {
        assert_eq!(read(address, offset), 0x12340000 + offset);
    }
    retract(mapping, 0);
    delete(raw_memo(mapping, 0).object);
    delete(CellObject::Slot(merged));
    delete(CellObject::Slot(a));
    let fresh = allocate(1, 1);
    let (mapping, address) = attach_large(fresh);
    for offset in offsets {
        assert_eq!(read(address, offset), 0);
    }
    retract(mapping, 0);
    delete(raw_memo(mapping, 0).object);
    delete(CellObject::Slot(fresh));
    let huge = allocate(2, 1);
    let (mapping, address) = reserve_huge(huge);
    let request = CellAssociation {
        source: slot(huge, 0),
        target: memo(mapping, 0),
    }
    .to_bytes();
    if cfg!(feature = "large") {
        cell::insert(ptr(&request)).unwrap();
        publish(mapping, 0);
        assert_eq!(read(address, (1 << 30) - 8), 0);
        write(address, 0, 0x17);
        write(address, (1 << 30) - 8, 0x39);
        retract(mapping, 0);
        delete(raw_memo(mapping, 0).object);
        let children = cell::split(huge).unwrap();
        absent(huge);
        assert_eq!(info(slot(children, 511 * 512)).level.get(), 1);
        let merged = cell::merge(children).unwrap();
        absent(children);
        let (mapping, address) = reserve_huge(merged);
        handoff(slot(merged, 0), memo(mapping, 0), false);
        publish(mapping, 0);
        assert_eq!(read(address, 0), 0x17);
        assert_eq!(read(address, (1 << 30) - 8), 0x39);
        retract(mapping, 0);
        delete(raw_memo(mapping, 0).object);
        delete(CellObject::Slot(merged));
        let _ = writeln!(
            Console,
            "PASS cell-gigabyte allocation=1 split=1 merge=1 data=1"
        );
    } else {
        assert_eq!(
            cell::insert(ptr(&request)).unwrap_err().source,
            kernel_api::abi::call::cell::CellFail::OutOfMemory
        );
        assert_eq!(info(slot(huge, 0)).state, CellState::Vacant);
        delete(raw_memo(mapping, 0).object);
        delete(CellObject::Slot(huge));
    }
    let request =
        CellCreate::Slot(Allocation::at_level(CellLevel::new(5).unwrap(), 1).unwrap()).to_bytes();
    assert!(cell::create(crate::license::get(), ptr(&request)).is_err());
    let _ = writeln!(
        Console,
        "PASS cell-levels contiguous=1 boundary=1 movement=1 split=1 merge=1 stale=1 zero=1 capacity=1"
    );
}
