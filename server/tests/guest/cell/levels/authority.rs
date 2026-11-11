use crate::{delete, info, ptr, publish, raw_memo, read, retract};
use kernel_api::{
    address::VirtAddr,
    model::{
        cell::*,
        flat::FlatInfo,
        identity::{Mark, PendId, TeamId},
        modifiable::Modifiable,
        pend::{AccordRequest, AccordTarget, NarrowRequest, PendRange},
    },
};
use kernel_client::call::{cell, flat, pend};
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
        limits.range = PendRange::Pages { offset, count };
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
    limits.range = PendRange::Pages {
        offset: 0,
        count: 512,
    };
    limits.modifiable = Modifiable::No;
    let child = pend::accord(ptr(&AccordRequest {
        src: root,
        target: AccordTarget::Caller,
        mark: Mark::NONE,
        limits,
    }
    .to_bytes()))
    .unwrap();
    let request = CellCreate::Memo {
        source: CellSource::Pend {
            token: child,
            offset: 0,
            count: 512,
        },
        destination: MemoDestination::Internal {
            team: TeamId::new(0),
            placement: Placement::lowest(4096).unwrap(),
            access: 2,
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
