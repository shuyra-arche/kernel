mod permission;
mod retirement;
mod share;

use kernel_api::model::unit::ExitScope;
use kernel_api::{
    abi::call::pend::PendFail,
    address::VirtAddr,
    model::{
        identity::{Mark, PendId, TaskId},
        modifiable::Modifiable,
        pend::{
            AccordRequest, AccordTarget, Limits, NarrowRequest, PendInfo, PendRange, PendStatus,
        },
        unit::UnitTarget,
    },
};
use kernel_client::call::{pend, share_view, unit};

fn accord(
    src: PendId,
    target: AccordTarget,
    subset: Modifiable,
    range: PendRange,
) -> kernel_api::abi::call::pend::PendResult<PendId> {
    let bytes = AccordRequest {
        src,
        target,
        limits: Limits {
            modifiable: subset,
            scope: crate::space::range(inspect(src).limits.scope, range),
            ..inspect(src).limits
        },
        mark: Mark::NONE,
    }
    .to_bytes();
    pend::accord(VirtAddr::new(bytes.as_ptr() as usize))
}

fn narrow(
    token: PendId,
    modifiable: Modifiable,
    range: PendRange,
) -> kernel_api::abi::call::pend::PendResult<()> {
    let bytes = NarrowRequest {
        token,
        limits: Limits {
            modifiable,
            scope: crate::space::range(inspect(token).limits.scope, range),
            ..inspect(token).limits
        },
    }
    .to_bytes();
    pend::narrow(super::ptr(&bytes))
}

pub(super) fn inspect(token: PendId) -> PendInfo {
    let mut words = [0; PendInfo::WORDS];
    pend::inspect(token, VirtAddr::new(words.as_mut_ptr() as usize)).unwrap();
    PendInfo::from_words(words).unwrap()
}

extern "C" fn recipient() -> ! {
    let mut words = [0; PendInfo::WORDS];
    assert_eq!(
        pend::collect(PendId::NONE, VirtAddr::new(words.as_mut_ptr() as usize), 1).unwrap(),
        1
    );
    let token = PendInfo::from_words(words).unwrap().token;
    let (address, len) = share_view::open(token).unwrap();
    assert_eq!(len, 4096);
    assert_eq!(super::read(address, 0), 0x13579);
    pend::return_(token).unwrap();
    unit::exit(0, 0, VirtAddr::new(0), 0, ExitScope::Task)
}

pub(super) fn run() {
    permission::run();
    retirement::run();
    share::run();
    let reference = super::allocate(8);
    let root = share_view::create(crate::license::get(), reference, Modifiable::Yes).unwrap();
    let (address, len) = share_view::open(root).unwrap();
    assert_eq!(len, 8 * 4096);
    super::write(address, 3 * 4096, 0x13579);
    let parent_address = address;

    let child = accord(
        root,
        AccordTarget::Caller,
        Modifiable::No,
        PendRange::pages(2, 4).unwrap(),
    )
    .unwrap();
    let leaf = accord(
        child,
        AccordTarget::Task(super::lifecycle::self_id()),
        Modifiable::No,
        PendRange::pages(1, 1).unwrap(),
    )
    .unwrap();
    assert_eq!(inspect(root).status, PendStatus::Active);
    assert_ne!(child, leaf);
    assert_eq!(inspect(child).target, inspect(leaf).target);
    assert_eq!(
        accord(
            child,
            AccordTarget::Caller,
            Modifiable::No,
            PendRange::pages(4, 1).unwrap()
        )
        .unwrap_err()
        .source,
        PendFail::Denied
    );
    assert_eq!(
        accord(
            child,
            AccordTarget::Caller,
            Modifiable::Yes,
            PendRange::Inherit
        )
        .unwrap_err()
        .source,
        PendFail::Denied
    );
    assert_eq!(
        accord(
            child,
            AccordTarget::Task(TaskId::new(usize::MAX)),
            Modifiable::No,
            PendRange::Inherit
        )
        .unwrap_err()
        .source,
        PendFail::Denied
    );
    assert_eq!(
        pend::accord(VirtAddr::new(usize::MAX)).unwrap_err().source,
        PendFail::Denied
    );
    let invalid = [0u8; AccordRequest::WIDTH];
    assert_eq!(
        pend::accord(VirtAddr::new(invalid.as_ptr() as usize))
            .unwrap_err()
            .source,
        PendFail::Invalid
    );

    let (address, len) = share_view::open(leaf).unwrap();
    assert_eq!(len, 4096);
    assert_eq!(super::read(address, 0), 0x13579);
    super::write(parent_address, 3 * 4096 + 8, 0x24680);
    assert_eq!(super::read(address, 8), 0x24680);
    let writer = accord(
        root,
        AccordTarget::Caller,
        Modifiable::Yes,
        PendRange::pages(3, 1).unwrap(),
    )
    .unwrap();
    let (alias, _) = share_view::open(writer).unwrap();
    super::write(alias, 8, 0x34567);
    assert_eq!(super::read(parent_address, 3 * 4096 + 8), 0x34567);
    assert_eq!(super::read(address, 8), 0x34567);
    pend::return_(writer).unwrap();
    super::write(parent_address, 3 * 4096 + 8, 0x45678);
    assert_eq!(super::read(address, 8), 0x45678);
    {
        use kernel_api::model::cell::*;
        use kernel_client::call::cell;
        let (mapping, _address) = crate::space::reserve(
            crate::space::get(),
            Placement::lowest(4096).unwrap(),
            1,
            CellLevel::BASE,
            2,
        );
        crate::handoff(crate::raw_memo(leaf, 0), crate::memo(mapping, 0), false);
        assert_eq!(
            super::info(super::raw_memo(mapping, 0)).state,
            CellState::Ready
        );
        let empty = super::allocate(1);
        let spec = CellHandoff {
            remote: super::memo(mapping, 0),
            local: super::slot(empty, 0),
        }
        .to_bytes();
        assert_eq!(
            cell::pull(super::ptr(&spec)).unwrap_err().source,
            kernel_api::abi::call::cell::CellFail::Denied
        );
        super::publish(mapping, 0);
        assert_eq!(super::read(_address, 0), 0x13579);
        let published = super::info(super::raw_memo(mapping, 0));
        let position = super::memo(mapping, 0).to_bytes();
        assert_eq!(
            cell::retract(super::ptr(&position)).unwrap_err().source,
            kernel_api::abi::call::cell::CellFail::Busy
        );
        assert_eq!(super::info(super::raw_memo(mapping, 0)), published);
        share_view::shut(root).unwrap();
        super::retract(mapping, 0);
        super::delete(super::raw_memo(mapping, 0).object);
        super::delete(CellObject::Slot(empty));
    }
    // Keep this mapping open: Return must withdraw the descendant mapping too.
    let mut words = [0; PendInfo::WORDS];
    let mut after = PendId::NONE;
    let mut found = 0;
    loop {
        let count = pend::collect(after, VirtAddr::new(words.as_mut_ptr() as usize), 1).unwrap();
        if count == 0 {
            break;
        }
        assert_eq!(count, 1);
        let info = PendInfo::from_words(words).unwrap();
        assert!(info.token.get() > after.get());
        if info.token == child || info.token == leaf {
            found += 1;
        }
        after = info.token;
    }
    assert_eq!(found, 2);
    // A distinct Task receives a range relative to this already narrowed child.
    let task = super::lifecycle::spawn(recipient as *const () as usize);
    let remote = accord(
        child,
        AccordTarget::Task(task),
        Modifiable::No,
        PendRange::pages(1, 1).unwrap(),
    )
    .unwrap();
    assert!(pend::inspect(remote, VirtAddr::new(words.as_mut_ptr() as usize)).is_err());
    unit::resume(UnitTarget::Task(task)).unwrap();
    assert!(super::lifecycle::wait_task(task));
    super::lifecycle::delete(task);
    assert_eq!(inspect(root).status, PendStatus::Active);
    assert_eq!(
        narrow(child, Modifiable::No, PendRange::pages(1, 1).unwrap())
            .unwrap_err()
            .source,
        PendFail::Busy
    );
    pend::return_(leaf).unwrap();
    narrow(child, Modifiable::No, PendRange::pages(1, 1).unwrap()).unwrap();
    assert_eq!(
        inspect(child).limits.scope.range(),
        Some(PendRange::pages(3, 1).unwrap())
    );
    let (narrowed, len) = share_view::open(child).unwrap();
    assert_eq!(len, 4096);
    assert_eq!(super::read(narrowed, 0), 0x13579);
    assert_eq!(
        narrow(child, Modifiable::No, PendRange::pages(0, 2).unwrap())
            .unwrap_err()
            .source,
        PendFail::Denied
    );
    pend::return_(child).unwrap();
    // Repeated registrations reuse internal slots while every returned public
    // identity stays invalid, including through Collect's sorted pagination.
    let mut previous = child;
    for _ in 0..32 {
        let next = accord(
            root,
            AccordTarget::Caller,
            Modifiable::No,
            PendRange::Inherit,
        )
        .unwrap();
        assert_ne!(next, previous);
        assert!(pend::inspect(previous, VirtAddr::new(words.as_mut_ptr() as usize)).is_err());
        assert!(share_view::open(previous).is_err());
        assert_eq!(inspect(next).status, PendStatus::Active);
        pend::return_(next).unwrap();
        previous = next;
    }
    assert!(pend::inspect(leaf, VirtAddr::new(words.as_mut_ptr() as usize)).is_err());
    assert_eq!(inspect(root).status, PendStatus::Active);
    let (address, _) = share_view::open(root).unwrap();
    super::write(address, 3 * 4096, 0x24680);
    assert_eq!(super::read(address, 3 * 4096), 0x24680);
    pend::return_(root).unwrap();
    super::delete(kernel_api::model::cell::CellObject::Slot(reference));
}
