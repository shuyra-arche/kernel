use kernel_api::model::memory::activity::*;
fn sample(sequence: u64) -> Sample {
    Sample {
        sequence,
        epoch: 2,
        nest: 1,
        reservation: 1,
        position: 0,
        generation: 3,
        page: 4,
        address: 0x8000,
        bytes: 4096,
        start_ns: 10,
        end_ns: 20,
        synchronized_ns: 30,
        validity: Validity::Observed,
        accessed: Evidence::Observed,
        dirty: Evidence::NotObserved,
    }
}
#[test]
fn sample_codec_preserves_identity_and_rejects_ambiguous_evidence() {
    let s = sample(1);
    assert_eq!(Sample::from_bytes(&s.to_bytes()), Some(s));
    for word in [0, 3, 4, 6, 7, 9] {
        let mut b = s.to_bytes();
        b[word * 8..word * 8 + 8].fill(0);
        assert!(Sample::from_bytes(&b).is_none());
    }
    let mut unknown = s;
    unknown.validity = Validity::Pending;
    assert!(Sample::from_bytes(&unknown.to_bytes()).is_none());
    unknown.accessed = Evidence::Unknown;
    assert!(Sample::from_bytes(&unknown.to_bytes()).is_none());
    unknown.dirty = Evidence::Unknown;
    assert_eq!(Sample::from_bytes(&unknown.to_bytes()), Some(unknown));
    let mut invalid = s;
    invalid.start_ns = 21;
    assert!(Sample::from_bytes(&invalid.to_bytes()).is_none());
    invalid = s;
    invalid.synchronized_ns = 19;
    assert!(Sample::from_bytes(&invalid.to_bytes()).is_none());
}
#[test]
fn coherent_sample_page_never_decodes_a_mixed_publication() {
    use std::sync::Arc;
    let p = Arc::new(SamplePage::new());
    assert!(p.read().is_none());
    let q = p.clone();
    let writer = std::thread::spawn(move || {
        for n in 1..10000 {
            let mut s = sample(n);
            s.page = n;
            s.generation = n;
            assert!(q.publish(s));
        }
    });
    for _ in 0..10000 {
        if let Some(s) = p.read() {
            assert_eq!(s.sequence, s.page);
            assert_eq!(s.page, s.generation);
        }
    }
    writer.join().unwrap();
    assert_eq!(core::mem::size_of::<SamplePage>(), 4096);
    assert_eq!(core::mem::align_of::<SamplePage>(), 4096);
}

#[test]
fn activity_reuses_existing_share_derive_and_keeps_all_new_nest_slots_closed() {
    use kernel_api::abi::{
        call::{EnvCall, share_view::operations::Derive},
        operation::RegisterCall,
    };
    assert_eq!(
        Derive::SLOT,
        ((kernel_api::abi::VERSION as usize) << 48) | (10 << 32) | 4
    );
    for slot in 0..64 {
        let number = ((kernel_api::abi::VERSION as usize) << 48) | (13 << 32) | slot;
        assert!(EnvCall::from_wire(number, &[0x1000, 0, 0, 0, 0, 0]).is_err());
    }
    assert!(limits(usize::MAX).is_none());
}
#[test]
fn pair_is_sign_then_share_and_rejects_forged_origin_writable_or_redistributable_views() {
    use kernel_api::model::{
        identity::{Mark, PendId, TaskId},
        modifiable::Modifiable,
        pend::*,
        sign::{SignId, SignRights},
        view::ViewId,
    };
    let id = |n: u64| PendId::from_bytes(&n.to_le_bytes()).unwrap();
    let view = PendInfo {
        token: id(7),
        target: Target::View(ViewTarget::Share(ViewId::new(1))),
        limits: limits(3).unwrap(),
        owner: TaskId::new(0),
        lender: TaskId::new(0),
        mark: Mark::NONE,
        status: PendStatus::Active,
    };
    let sign = PendInfo {
        token: id(8),
        target: Target::Sign(SignId::new(2)),
        limits: Limits {
            operations: Operations::Sign(SignRights::LISTEN | SignRights::CONSUME),
            modifiable: Modifiable::No,
            distributable: Distributable::No,
            scope: PendScope::Object,
        },
        mark: Mark::new(7),
        ..view
    };
    assert_eq!(pair(sign, view), Some((id(8), id(7))));
    assert!(pair(view, sign).is_none());
    assert!(
        pair(
            PendInfo {
                mark: Mark::new(6),
                ..sign
            },
            view
        )
        .is_none()
    );
    assert!(
        pair(
            PendInfo {
                owner: TaskId::new(9),
                ..sign
            },
            view
        )
        .is_none()
    );
    assert!(
        pair(
            PendInfo {
                lender: TaskId::new(9),
                ..sign
            },
            view
        )
        .is_none()
    );
    assert!(
        pair(
            sign,
            PendInfo {
                limits: Limits {
                    modifiable: Modifiable::Yes,
                    ..view.limits
                },
                ..view
            }
        )
        .is_none()
    );
    assert!(
        pair(
            sign,
            PendInfo {
                limits: Limits {
                    distributable: Distributable::Yes,
                    ..view.limits
                },
                ..view
            }
        )
        .is_none()
    );
}
