use super::*;

pub(super) fn run() {
    use kernel_api::model::clone_view::CloneLimits;
    use kernel_api::model::pend::{CloneRights, Distributable, Operations, ShareRights};
    use kernel_client::call::clone_view;
    let reference = super::super::allocate(1);
    let root = share_view::create(crate::license::get(), reference, Modifiable::Yes).unwrap();
    let limits = Limits {
        operations: Operations::Share(ShareRights::OPEN | ShareRights::SHUT),
        modifiable: Modifiable::No,
        distributable: Distributable::No,
        range: PendRange::Inherit,
    };
    let bytes = AccordRequest {
        src: root,
        target: AccordTarget::Caller,
        limits,
        mark: Mark::NONE,
    }
    .to_bytes();
    let child = pend::accord(super::super::ptr(&bytes)).unwrap();
    assert_eq!(inspect(child).limits.distributable, Distributable::No);
    share_view::open(child).unwrap();
    share_view::shut(child).unwrap();
    assert_eq!(
        accord(
            child,
            AccordTarget::Caller,
            Modifiable::No,
            PendRange::Inherit
        )
        .unwrap_err()
        .source,
        PendFail::Denied
    );
    let bytes = NarrowRequest {
        token: child,
        limits: Limits {
            distributable: Distributable::Yes,
            ..limits
        },
    }
    .to_bytes();
    assert_eq!(
        pend::narrow(super::super::ptr(&bytes)).unwrap_err().source,
        PendFail::Denied
    );
    let bytes = NarrowRequest {
        token: child,
        limits: Limits {
            operations: Operations::Clone(CloneRights::PUSH),
            ..limits
        },
    }
    .to_bytes();
    assert_eq!(
        pend::narrow(super::super::ptr(&bytes)).unwrap_err().source,
        PendFail::Denied
    );
    let bytes = NarrowRequest {
        token: child,
        limits: Limits {
            operations: Operations::Share(ShareRights::SHUT),
            ..limits
        },
    }
    .to_bytes();
    pend::narrow(super::super::ptr(&bytes)).unwrap();
    assert!(share_view::open(child).is_err());
    pend::return_(child).unwrap();
    // The old owning Slot must not bypass a later restriction on its root.
    let bytes = NarrowRequest {
        token: root,
        limits: Limits {
            operations: Operations::Share(ShareRights::OPEN | ShareRights::SHUT),
            ..inspect(root).limits
        },
    }
    .to_bytes();
    pend::narrow(super::super::ptr(&bytes)).unwrap();
    use kernel_api::model::{cell::*, identity::TeamId};
    let bytes = CellCreate::Memo {
        source: CellSource::Slot(ReferenceRange {
            reference,
            offset: 0,
            count: 1,
        }),
        destination: MemoDestination::Internal {
            team: TeamId::new(0),
            placement: Placement::lowest(4096).unwrap(),
            access: 2,
        },
    }
    .to_bytes();
    assert!(
        kernel_client::call::cell::create(
            kernel_api::model::identity::PendId::NONE,
            super::super::ptr(&bytes)
        )
        .is_err()
    );
    share_view::open(root).unwrap();
    share_view::shut(root).unwrap();
    pend::return_(root).unwrap();
    super::super::delete(kernel_api::model::cell::CellObject::Slot(reference));

    let root =
        clone_view::create(crate::license::get(), Mark::NONE, CloneLimits::default()).unwrap();
    let bytes = AccordRequest {
        src: root,
        target: AccordTarget::Caller,
        limits: Limits {
            operations: Operations::Clone(CloneRights::PEEK),
            modifiable: Modifiable::No,
            distributable: Distributable::No,
            range: PendRange::Inherit,
        },
        mark: Mark::NONE,
    }
    .to_bytes();
    let peek = pend::accord(super::super::ptr(&bytes)).unwrap();
    let message = 0x8172615041326354u64;
    clone_view::push(root, super::super::ptr(&message.to_le_bytes()), 8).unwrap();
    assert_eq!(clone_view::peek(peek).unwrap().0, 8);
    assert_eq!(
        clone_view::pull(peek, 4096).unwrap_err().source,
        kernel_api::abi::call::clone_view::CloneFail::Denied
    );
    assert_eq!(
        clone_view::push(peek, super::super::ptr(&message.to_le_bytes()), 8)
            .unwrap_err()
            .source,
        kernel_api::abi::call::clone_view::CloneFail::Denied
    );
    let delivery = clone_view::pull(root, 4096).unwrap();
    let (address, _) = share_view::open(delivery.token).unwrap();
    assert_eq!(super::super::read(address, 0), message as usize);
    pend::return_(delivery.token).unwrap();
    pend::return_(peek).unwrap();
    let bytes = AccordRequest {
        src: root,
        target: AccordTarget::Caller,
        limits: Limits {
            operations: Operations::Clone(CloneRights::DELETE),
            modifiable: Modifiable::No,
            distributable: Distributable::No,
            range: PendRange::Inherit,
        },
        mark: Mark::NONE,
    }
    .to_bytes();
    let delete = pend::accord(super::super::ptr(&bytes)).unwrap();
    clone_view::delete(delete).unwrap();
    pend::return_(delete).unwrap();
    pend::return_(root).unwrap();
}
