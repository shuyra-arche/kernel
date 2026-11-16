use kernel_api::{
    address::{PageCount, VirtAddr},
    model::{
        flat::{FlatId, FlatInfo},
        nest::{NestId, NestInfo, NestKind, NestPhase},
    },
};

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
fn native_memory_never_reuses_retired_register_numbers() {
    use kernel_api::abi::{PREFIX, call::EnvCall};
    for class in [2usize, 12, 13, 14] {
        for slot in 0..64 {
            assert!(EnvCall::from_wire(PREFIX | (class << 32) | slot, &[1; 6]).is_err());
        }
    }
    for slot in [0, 2, 3, 5, 6] {
        assert!(EnvCall::from_wire(PREFIX | (10usize << 32) | slot, &[1; 6]).is_err());
    }
}
