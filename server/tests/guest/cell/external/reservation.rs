//! External reservation geometry and scoped completion authorization.
use super::*;
use core::fmt::Write;
use kernel_api::{abi::call::sign::SignFail, model::pend::PendRange};

pub(super) fn run(root: PendId) {
    let placement = Placement::exact(VirtAddr::new(ADDRESS), 4096).unwrap();
    for (placement, count, level, access) in [
        (Placement::lowest(4096).unwrap(), 1, CellLevel::BASE, 2),
        (Placement::highest(), 1, CellLevel::BASE, 2),
        (
            Placement::exact(VirtAddr::new(0), 4096).unwrap(),
            1,
            CellLevel::BASE,
            2,
        ),
        (
            Placement::exact(VirtAddr::new(1 << 38), 4096).unwrap(),
            1,
            CellLevel::BASE,
            2,
        ),
        (placement, 2, CellLevel::BASE, 2),
        (
            Placement::exact(VirtAddr::new(1 << 30), 1 << 30).unwrap(),
            1 << 18,
            CellLevel::new(2).unwrap(),
            2,
        ),
        (placement, 1, CellLevel::BASE, 10),
    ] {
        assert!(
            nest::reserve(
                root,
                placement,
                PageCount::new(count).unwrap(),
                level,
                access
            )
            .is_err()
        );
    }
    let (vacant, address) = nest::reserve(
        root,
        placement,
        PageCount::new(1).unwrap(),
        CellLevel::BASE,
        2,
    )
    .unwrap();
    assert_eq!(address.get(), ADDRESS);
    let position = crate::memo(vacant, 0);
    let observation = crate::info(position);
    assert_eq!(observation.state, CellState::Vacant);
    assert_eq!(observation.level, CellLevel::BASE);
    assert_eq!(observation.page, 0);
    assert!(cell::publish(crate::ptr(&position.to_bytes())).is_err());
    assert_eq!(crate::info(position), observation);
    let (other, _) = nest::reserve(
        root,
        Placement::exact(VirtAddr::new(ADDRESS + 4096), 4096).unwrap(),
        PageCount::new(1).unwrap(),
        CellLevel::BASE,
        2,
    )
    .unwrap();
    let mapping = crate::space::id(vacant);
    let other_id = crate::space::id(other);
    let no_inspect = crate::space::accord(
        vacant,
        AccordTarget::Caller,
        Limits {
            operations: Operations::Nest(NestRights::RETRACT),
            modifiable: Modifiable::No,
            distributable: Distributable::No,
            scope: crate::space::mapping(vacant, PendRange::Inherit),
        },
    );
    let endpoint = sign::create(crate::license::get()).unwrap();
    for (token, mapping) in [(root, mapping), (vacant, other_id), (no_inspect, mapping)] {
        for source in [
            SignSource::MappingPublished { token, mapping },
            SignSource::MappingRetired { token, mapping },
        ] {
            assert_eq!(
                sign::attach(endpoint, source, 0).unwrap_err().source,
                SignFail::Denied
            );
        }
    }
    sign::delete(endpoint).unwrap();
    returned(endpoint);
    returned(no_inspect);
    for token in [vacant, other] {
        crate::delete(CellObject::Pend(token));
        returned(token);
    }

    let private = crate::allocate(2);
    let position = create(root, private, ADDRESS, true);
    assert_eq!(crate::info(position).state, CellState::Ready);
    assert!(cell::publish(crate::ptr(&current(position).to_bytes())).is_err());
    assert_eq!(crate::info(position).state, CellState::Ready);
    assert!(!crate::info(position).external);
    crate::delete(position.object);
    let CellObject::Pend(token) = position.object else {
        unreachable!()
    };
    returned(token);
    crate::delete(CellObject::Slot(private));
    writeln!(
        crate::Console,
        "PASS external-reservation sourceless=1 geometry=1 private-window=1 sign-scope=1"
    )
    .unwrap();
}
