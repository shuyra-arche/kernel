use crate::authority::Pend;
use kernel_api::{
    abi::call::pend::PendFail,
    address::{PageCount, PageOffset},
    model::{
        cell::MappingId,
        modifiable::Modifiable::{No, Yes},
        pend::{Distributable, Limits, NestRights, NestScope, Operations, PendRange, PendScope},
    },
};
use std::sync::{Arc, Barrier};

fn mapping(id: usize, offset: usize, count: usize) -> PendScope {
    PendScope::Nest(NestScope::Mapping {
        id: MappingId::from_bytes(&(id as u64).to_le_bytes()).unwrap(),
        range: PendRange::Pages { offset, count },
    })
}

fn authority() -> Arc<Pend> {
    Pend::try_new_scope(
        Yes,
        mapping(1, 10, 8),
        Operations::Nest(NestRights::mapping()),
        Distributable::Yes,
    )
    .unwrap()
}

fn reduced() -> Limits {
    Limits {
        modifiable: No,
        operations: Operations::Nest(NestRights::INSPECT),
        distributable: Distributable::No,
        scope: mapping(1, 12, 2),
    }
}

#[test]
fn limits_keep_scope_identity_and_reject_absolute_window_widening() {
    let authority = authority();
    let original = authority.limits();
    for scope in [
        PendScope::Object,
        PendScope::Pages(PendRange::Pages {
            offset: 12,
            count: 2,
        }),
        PendScope::Nest(NestScope::Whole),
        mapping(2, 12, 2),
        mapping(1, 9, 2),
        mapping(1, 17, 2),
        PendScope::Nest(NestScope::Mapping {
            id: MappingId::from_bytes(&1u64.to_le_bytes()).unwrap(),
            range: PendRange::Inherit,
        }),
    ] {
        let next = Limits { scope, ..reduced() };
        assert_eq!(authority.can_narrow(next), Err(PendFail::Denied));
        assert_eq!(authority.narrow(next), Err(PendFail::Denied));
        assert_eq!(authority.limits(), original);
    }
    authority.narrow(reduced()).unwrap();
    assert_eq!(authority.scope(), mapping(1, 12, 2));
    assert_eq!(
        authority.window(),
        Some((PageOffset::new(12), PageCount::new(2).unwrap()))
    );
    assert!(authority.contains(PageOffset::new(12), PageCount::new(2).unwrap()));
    assert!(!authority.contains(PageOffset::new(11), PageCount::new(2).unwrap()));
    assert_eq!(authority.narrow(original), Err(PendFail::Denied));
}

#[test]
fn prepared_limit_drop_rolls_back_and_commit_updates_all_limits() {
    let authority = authority();
    let original = authority.limits();
    let prepared = authority.prepare_limit(reduced()).unwrap();
    drop(prepared);
    assert_eq!(authority.limits(), original);
    let prepared = authority.prepare_limit(reduced()).unwrap();
    prepared.commit();
    assert_eq!(authority.limits(), reduced());
    assert_eq!(
        authority.prepare_limit(original).err(),
        Some(PendFail::Denied)
    );
    assert_eq!(authority.limits(), reduced());
}

#[test]
fn prepared_limit_checks_live_ancestry_and_waits_for_exact_publisher_route() {
    let parent = Pend::try_new_scope(
        Yes,
        PendScope::Nest(NestScope::Whole),
        Operations::Nest(NestRights::whole()),
        Distributable::Yes,
    )
    .unwrap();
    let child = authority();
    child.inherit(&parent).unwrap();
    child.retain_route().unwrap();
    assert_eq!(child.can_narrow(reduced()), Ok(()));
    assert_eq!(child.prepare_limit(reduced()).err(), Some(PendFail::Busy));
    unsafe { child.release_route() };
    drop(child.prepare_limit(reduced()).unwrap());
    parent.invalidate();
    assert_eq!(child.prepare_limit(reduced()).err(), Some(PendFail::Dead));
}

#[test]
fn concurrent_limits_observe_one_state_not_a_mixture_of_scope_and_rights() {
    let authority = authority();
    let original = authority.limits();
    let barrier = Arc::new(Barrier::new(2));
    let observed = authority.clone();
    let ready = barrier.clone();
    let reader = std::thread::spawn(move || {
        ready.wait();
        for _ in 0..4096 {
            let limits = observed.limits();
            assert!(limits == original || limits == reduced());
        }
    });
    barrier.wait();
    authority.prepare_limit(reduced()).unwrap().commit();
    reader.join().unwrap();
    assert_eq!(authority.limits(), reduced());
}
