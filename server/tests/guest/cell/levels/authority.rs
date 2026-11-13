use crate::{delete, info, ptr, publish, raw_memo, read, retract};
use kernel_api::{
    address::VirtAddr,
    model::{
        cell::*,
        flat::FlatInfo,
        identity::Mark,
        modifiable::Modifiable,
        pend::{AccordRequest, AccordTarget, NarrowRequest, PendRange, PendScope},
    },
};
use kernel_client::call::{flat, pend};
pub(super) fn run() {
    let root = flat::create(crate::license::get(), 1 << 21, 1 << 21, 1).unwrap();
    let mut out = [0; FlatInfo::WIDTH];
    flat::inspect(root, VirtAddr::new(out.as_mut_ptr() as usize)).unwrap();
    let snapshot = FlatInfo::from_bytes(&out).unwrap();
    assert_eq!(snapshot.level.get(), 1);
    assert_eq!(snapshot.count, 512);
    assert!(flat::slice(root, 0, 1).is_err());
    assert!(flat::slice(root, 1, 511).is_err());
    let original = crate::pend::inspect(root).limits;
    for (offset, count) in [(0, 1), (1, 511)] {
        let mut limits = original;
        limits.scope = PendScope::Pages(PendRange::Pages { offset, count });
        let request = AccordRequest {
            src: root,
            target: AccordTarget::Caller,
            mark: Mark::NONE,
            limits,
        }
        .to_bytes();
        assert!(pend::accord(ptr(&request)).is_err());
        assert!(
            pend::narrow(ptr(&NarrowRequest {
                token: root,
                limits
            }
            .to_bytes()))
            .is_err()
        );
        assert_eq!(crate::pend::inspect(root).limits, original);
    }
    let mut limits = original;
    limits.scope = PendScope::Pages(PendRange::Pages {
        offset: 0,
        count: 512,
    });
    limits.modifiable = Modifiable::No;
    let child = pend::accord(ptr(&AccordRequest {
        src: root,
        target: AccordTarget::Caller,
        mark: Mark::NONE,
        limits,
    }
    .to_bytes()))
    .unwrap();
    let (mapping, address) = crate::space::reserve(
        crate::space::get(),
        Placement::lowest(1 << 21).unwrap(),
        512,
        CellLevel::new(1).unwrap(),
        2,
    );
    crate::handoff(raw_memo(child, 0), crate::memo(mapping, 0), false);
    assert_eq!(info(raw_memo(mapping, 0)).state, CellState::Ready);
    assert_eq!(info(raw_memo(mapping, 0)).level.get(), 1);
    publish(mapping, 0);
    assert_eq!(read(address, (1 << 21) - 8), 0);
    retract(mapping, 0);
    delete(raw_memo(mapping, 0).object);
    pend::return_(child).unwrap();
    flat::delete(root).unwrap();
    pend::return_(root).unwrap();
}
