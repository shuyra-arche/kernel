//! Test-side selection and delegation of explicit Nest capabilities.
use kernel_api::{
    address::{PageCount, VirtAddr},
    boot::{
        args,
        ledger::{Entries, Name, ResourceKind},
    },
    model::{
        cell::{CellLevel, Placement},
        identity::{Mark, PendId, TaskId},
        nest::{NestInfo, NestKind},
        pend::{
            AccordRequest, AccordTarget, Distributable, Limits, NestRights, NestScope, Operations,
            PendInfo, PendRange, PendScope,
        },
    },
};
use kernel_client::call::{nest, pend};

mod association;
mod protection;
mod retirement;
mod scope;
mod source;

pub(crate) fn run() {
    scope::run();
    association::run();
    protection::run();
    retirement::run();
    source::run();
}

pub(crate) fn accord(src: PendId, target: AccordTarget, limits: Limits) -> PendId {
    let bytes = AccordRequest {
        src,
        target,
        limits,
        mark: Mark::NONE,
    }
    .to_bytes();
    pend::accord(crate::ptr(&bytes)).unwrap()
}

pub(crate) fn id(token: PendId) -> kernel_api::model::nest::ReservationId {
    let PendScope::Nest(NestScope::Reservation { id, .. }) = crate::pend::inspect(token).limits.scope
    else {
        panic!("mapping scope expected")
    };
    id
}

pub(crate) fn boot(at: usize, count: usize) {
    assert_eq!(count, args::LEN);
    let words = unsafe { core::slice::from_raw_parts(at as *const usize, count) };
    let bytes = unsafe {
        core::slice::from_raw_parts(words[args::LEDGER] as *const u8, words[args::LEDGER_LEN])
    };
    let entry = Entries::new(bytes).unwrap().find(Name::Space).unwrap();
    assert_eq!(entry.kind(), ResourceKind::Nest);
    let token = PendId::mint(entry.handle());
    let info = crate::pend::inspect(token);
    assert_eq!(info.limits.scope, PendScope::Nest(NestScope::Whole));
    assert!(
        info.limits
            .operations
            .contains(Operations::Nest(NestRights::INSPECT | NestRights::RESERVE))
    );
    assert_eq!(get(), token);
}

pub(crate) fn find() -> Option<PendId> {
    let mut after = PendId::NONE;
    loop {
        let mut words = [0; PendInfo::WORDS];
        if pend::collect(after, VirtAddr::new(words.as_mut_ptr() as usize), 1).unwrap() == 0 {
            return None;
        }
        let info = PendInfo::from_words(words).unwrap();
        if info.limits.scope == PendScope::Nest(NestScope::Whole)
            && info
                .limits
                .operations
                .contains(Operations::Nest(NestRights::INSPECT | NestRights::RESERVE))
        {
            let mut bytes = [0; NestInfo::WIDTH];
            nest::inspect(info.token, VirtAddr::new(bytes.as_mut_ptr() as usize)).unwrap();
            if NestInfo::from_bytes(&bytes).unwrap().kind == NestKind::Internal {
                return Some(info.token);
            }
        }
        after = info.token;
    }
}

pub(crate) fn get() -> PendId {
    find().expect("Task needs an explicitly installed Whole RESERVE grant")
}

pub(crate) fn delegate(root: PendId, task: TaskId) -> PendId {
    let parent = crate::pend::inspect(root).limits;
    let bytes = AccordRequest {
        src: root,
        target: AccordTarget::Task(task),
        mark: Mark::NONE,
        limits: Limits {
            operations: Operations::Nest(NestRights::INSPECT | NestRights::RESERVE),
            modifiable: parent.modifiable,
            distributable: Distributable::No,
            scope: PendScope::Nest(NestScope::Whole),
        },
    }
    .to_bytes();
    pend::accord(crate::ptr(&bytes)).unwrap()
}

pub(crate) fn reserve(
    whole: PendId,
    placement: Placement,
    count: usize,
    level: CellLevel,
    access: u64,
) -> (PendId, VirtAddr) {
    nest::reserve(
        whole,
        placement,
        PageCount::new(count).unwrap(),
        level,
        access,
    )
    .unwrap()
}

pub(crate) fn range(scope: PendScope, range: PendRange) -> PendScope {
    match scope {
        PendScope::Pages(_) => PendScope::Pages(range),
        PendScope::Nest(NestScope::Reservation { id, .. }) => {
            PendScope::Nest(NestScope::Reservation { id, range })
        }
        PendScope::Object | PendScope::Nest(NestScope::Whole) => {
            assert_eq!(range, PendRange::Inherit);
            scope
        }
    }
}

pub(crate) fn mapping(token: PendId, range: PendRange) -> PendScope {
    let scope = crate::pend::inspect(token).limits.scope;
    assert!(matches!(scope, PendScope::Nest(NestScope::Reservation { .. })));
    self::range(scope, range)
}
