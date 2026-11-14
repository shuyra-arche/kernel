use kernel_api::{
    abi::{
        self,
        call::{EnvCall, flat::operations as flat, nest::operations as nest},
        operation::RegisterCall,
    },
    address::{PageCount, VirtAddr},
    model::{
        flat::{FlatId, FlatInfo},
        identity::PendId,
        nest::{NestId, NestInfo, NestKind, NestPhase},
    },
};
#[test]
fn runtime_calls_keep_distinct_domains_and_reject_retired_bindings() {
    let license = PendId::mint(7);
    let create = flat::Create {
        license,
        size: 8192,
        align: 4096,
        level: 0,
    };
    assert_eq!(flat::Create::SLOT, abi::PREFIX | (12 << 32));
    assert!(EnvCall::from_wire(flat::Create::SLOT, &create.pack()).is_err());
    let create = nest::Create { license };
    assert!(EnvCall::from_wire(nest::Create::SLOT, &create.pack()).is_err());
    let inspect = nest::Inspect {
        token: license,
        out: VirtAddr::new(4096),
    };
    assert!(EnvCall::from_wire(nest::Inspect::SLOT, &inspect.pack()).is_err());
    for number in [
        abi::PREFIX | (14 << 32),
        abi::PREFIX | (14 << 32) | 1,
        abi::PREFIX | (13 << 32) | 2,
        abi::PREFIX | (13 << 32) | 3,
        (5usize << 48) | (13 << 32),
        (5usize << 48) | (14 << 32),
    ] {
        assert!(EnvCall::from_wire(number, &[7, 8, 0, 0, 0, 0]).is_err());
    }
}
#[test]
fn organization_snapshots_reject_unknown_identity_geometry_and_phase() {
    let flat = FlatInfo {
        level: kernel_api::model::cell::CellLevel::BASE,
        id: FlatId::new(7).unwrap(),
        offset: 2,
        count: 3,
    };
    assert_eq!(FlatInfo::from_bytes(&flat.to_bytes()), Some(flat));
    let mut bad = flat.to_bytes();
    bad[..8].fill(0);
    assert!(FlatInfo::from_bytes(&bad).is_none());
    bad = flat.to_bytes();
    bad[16..24].fill(0);
    assert!(FlatInfo::from_bytes(&bad).is_none());
    let nest = NestInfo {
        id: NestId::new(8).unwrap(),
        kind: NestKind::External,
        phase: NestPhase::Closing,
        fault_sequence: 2,
        fault_cause: 13,
        fault_address: 0x4000,
        fault_requester: 8,
    };
    assert_eq!(NestInfo::WIDTH, 56);
    assert_eq!(NestInfo::from_bytes(&nest.to_bytes()), Some(nest));
    for (at, value) in [(0, 0), (8, 2), (16, 5)] {
        let mut bad = nest.to_bytes();
        bad[at] = value;
        assert!(NestInfo::from_bytes(&bad).is_none());
    }
    for len in 0..NestInfo::WIDTH {
        assert!(NestInfo::from_bytes(&nest.to_bytes()[..len]).is_none());
    }
}

#[test]
fn reserve_snapshot_is_source_free_and_checks_count_level_placement_and_access() {
    use super::fetch_words;
    use kernel_api::model::{
        cell::{CellLevel, Placement},
        nest::ReserveRequest,
    };
    assert_eq!(ReserveRequest::WIDTH, 48);
    let request = ReserveRequest {
        placement: Placement::exact(VirtAddr::new(0x8000), 4096).unwrap(),
        count: PageCount::new(1024).unwrap(),
        level: CellLevel::new(1).unwrap(),
        access: 6,
    };
    assert!(request.valid());
    assert_eq!(
        fetch_words::<6>(&request.to_bytes()).unwrap(),
        [2, 0x8000, 4096, 1024, 1, 6]
    );
    for placement in [
        Placement::lowest(4096).unwrap(),
        Placement::highest(),
        request.placement,
    ] {
        for access in [2, 6, 8, 10, 14] {
            let value = ReserveRequest {
                placement,
                access,
                ..request
            };
            assert_eq!(ReserveRequest::from_bytes(&value.to_bytes()), Some(value));
        }
    }
    for (word, invalid) in [
        (0, 3usize),
        (1, 0x8001),
        (2, 0),
        (2, 2048),
        (2, 6144),
        (3, 0),
        (3, 1),
        (3, 513),
        (3, usize::MAX / 4096 + 1),
        (4, 6),
        (4, usize::MAX),
        (5, 0),
        (5, 1),
        (5, 4),
        (5, 12),
        (5, 16),
        (5, usize::MAX),
    ] {
        let mut bytes = request.to_bytes();
        bytes[word * 8..(word + 1) * 8].copy_from_slice(&invalid.to_le_bytes());
        assert_eq!(
            ReserveRequest::from_bytes(&bytes),
            None,
            "word {word}, value {invalid}"
        );
    }
    for count in [1, 513, usize::MAX / 4096 + 1] {
        assert!(
            !ReserveRequest {
                count: PageCount::new(count).unwrap(),
                ..request
            }
            .valid()
        );
    }
    for access in [0, 1, 4, 12, 16, u64::MAX] {
        assert!(!ReserveRequest { access, ..request }.valid());
    }
    let mut bytes = ReserveRequest {
        placement: Placement::lowest(4096).unwrap(),
        ..request
    }
    .to_bytes();
    bytes[8] = 1;
    assert_eq!(ReserveRequest::from_bytes(&bytes), None);
    for len in 0..ReserveRequest::WIDTH {
        assert_eq!(ReserveRequest::from_bytes(&request.to_bytes()[..len]), None);
    }
    assert_eq!(ReserveRequest::from_bytes(&[0; 49]), None);
    // Logical Cell level is not a hardware-leaf alignment requirement.
    assert!(
        ReserveRequest {
            placement: Placement::exact(VirtAddr::new(4096), 4096).unwrap(),
            ..request
        }
        .valid()
    );
}

#[test]
fn reserve_and_release_have_additive_slots_pair_reply_and_stable_failures() {
    use kernel_api::abi::{call::nest::NestFail, operation::KernelCall};
    let token = PendId::mint(7);
    let reserve = nest::Reserve {
        token,
        request: VirtAddr::new(0x4000),
    };
    assert_eq!(nest::Reserve::SLOT, abi::PREFIX | (13 << 32) | 5);
    assert_eq!(reserve.pack(), [7, 0x4000, 0, 0, 0, 0]);
    assert!(EnvCall::from_wire(nest::Reserve::SLOT, &reserve.pack()).is_err());
    assert_eq!(
        nest::Reserve::decode_return((11, 0x8000, 99)).unwrap(),
        (PendId::mint(11), VirtAddr::new(0x8000))
    );
    let release = nest::Release { token };
    assert_eq!(nest::Release::SLOT, abi::PREFIX | (13 << 32) | 6);
    assert_eq!(release.pack(), [7, 0, 0, 0, 0, 0]);
    assert!(EnvCall::from_wire(nest::Release::SLOT, &release.pack()).is_err());
    assert_eq!(nest::Release::decode_return((0, 0, 0)).unwrap(), ());
    for (fail, code) in [
        (NestFail::Denied, -1),
        (NestFail::Dead, -2),
        (NestFail::Busy, -3),
        (NestFail::OutOfMemory, -4),
        (NestFail::NotAligned, -5),
        (NestFail::AlreadyMapped, -6),
        (NestFail::NoRegion, -7),
    ] {
        assert_eq!(fail.code(), code);
        assert_eq!(NestFail::of_code(code), Some(fail));
        assert_eq!(
            nest::Reserve::decode_return((code as usize, 0, 0))
                .unwrap_err()
                .source,
            fail
        );
        assert_eq!(
            nest::Release::decode_return((code as usize, 0, 0))
                .unwrap_err()
                .source,
            fail
        );
    }
    assert!(NestFail::Busy.is_busy());
    for reply in [
        (0, 0x8000, 0),
        (isize::MAX as usize + 1, 0x8000, 0),
        (11, 1, 0),
    ] {
        assert!(std::panic::catch_unwind(|| nest::Reserve::decode_return(reply)).is_err());
    }
}
