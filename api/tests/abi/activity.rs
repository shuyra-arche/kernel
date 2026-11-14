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
fn observation_is_an_additive_bounded_descriptor_without_reopening_old_memory_slots() {
    use kernel_api::{
        abi::{
            call::{
                EnvCall,
                nest::{NestCall, operations::Observe},
            },
            operation::RegisterCall,
        },
        address::VirtAddr,
        model::{
            identity::PendId,
            nest::{MemoGeneration, MemoPosition, NestId, ReservationId},
        },
    };
    let input = VirtAddr::new(0x10000);
    assert!(
        matches!(EnvCall::from_wire(Observe::SLOT,&[input.get(),0,0,0,0,0]),
        Ok(EnvCall::Nest(NestCall::Observe {request})) if request==input)
    );
    for slot in 0..27 {
        let number = ((kernel_api::abi::VERSION as usize) << 48) | (13 << 32) | slot;
        assert!(EnvCall::from_wire(number, &[input.get(), 0, 0, 0, 0, 0]).is_err());
    }
    let position = MemoPosition::new(
        PendId::from_bytes(&1u64.to_le_bytes()).unwrap(),
        NestId::new(2).unwrap(),
        ReservationId::from_bytes(&3u64.to_le_bytes()).unwrap(),
        0,
    )
    .unwrap();
    let request = Request {
        position,
        generation: MemoGeneration::new(4).unwrap(),
        period_ns: MIN_PERIOD_NS,
    };
    assert_eq!(Request::from_bytes(&request.to_bytes()), Some(request));
    let mut bad = request.to_bytes();
    bad[48..].copy_from_slice(&(VERSION + 1).to_le_bytes());
    assert!(Request::from_bytes(&bad).is_none());
    for period in [0, MIN_PERIOD_NS - 1, MAX_PERIOD_NS + 1, u64::MAX] {
        let bad = Request {
            period_ns: period,
            ..request
        }
        .to_bytes();
        assert!(Request::from_bytes(&bad).is_none());
    }
    assert!(Request::from_bytes(&request.to_bytes()[..48]).is_none());
}
