use kernel_api::{
    address::VirtAddr,
    model::{
        cell::*,
        identity::{PendId, TeamId},
    },
};
#[test]
fn positions_reject_bad_identity_reserved_words_and_stale_slot_tags() {
    let slot = SlotId::from_bytes(&1u64.to_le_bytes()).unwrap();
    let mapping = MappingId::from_bytes(&7u64.to_le_bytes()).unwrap();
    for object in [
        CellObject::Slot(slot),
        CellObject::Memo {
            target: MappingTarget::Cpu(TeamId::new(0)),
            mapping,
        },
        CellObject::Memo {
            target: MappingTarget::Io(PendId::mint(2)),
            mapping,
        },
    ] {
        let p = CellPosition {
            object,
            page: 3,
            generation: if matches!(object, CellObject::Slot(_)) {
                0
            } else {
                u64::MAX
            },
        };
        assert_eq!(CellPosition::from_bytes(&p.to_bytes()), Some(p));
        assert_eq!(CellObject::from_bytes(&object.to_bytes()), Some(object));
    }
    let p = CellPosition {
        object: CellObject::Slot(slot),
        page: 0,
        generation: 0,
    };
    let mut bad = p.to_bytes();
    bad[40] = 1;
    assert!(CellPosition::from_bytes(&bad).is_none());
    let mut bad = p.to_bytes();
    bad[16] = 1;
    assert!(CellPosition::from_bytes(&bad).is_none());
    let mut bad = p.to_bytes();
    bad[8] = 0;
    assert!(CellPosition::from_bytes(&bad).is_none());
    let mut bad = p.to_bytes();
    bad[32..40].fill(255);
    assert!(CellPosition::from_bytes(&bad).is_none());
    assert!(CellPosition::from_bytes(&[0; 47]).is_none());
    let request = CellHandoff {
        source: p,
        target: p,
    };
    assert_eq!(CellHandoff::from_bytes(&request.to_bytes()), Some(request));
    assert!(CellHandoff::from_bytes(&request.to_bytes()[..95]).is_none());
}
#[test]
fn creation_is_one_canonical_snapshot_for_geometry_and_authority() {
    for source in [
        CellSource::Slot(ReferenceRange {
            reference: SlotId::from_bytes(&3u64.to_le_bytes()).unwrap(),
            offset: 0,
            count: 1,
        }),
        CellSource::Pend {
            token: PendId::mint(4),
            offset: 2,
            count: 1,
        },
    ] {
        let spec = CellCreate::Memo {
            source,
            destination: MemoDestination::Internal {
                team: TeamId::new(0),
                placement: Placement::exact(VirtAddr::new(0x8000), 4096).unwrap(),
                access: 6,
                control: MappingControl::Caller,
            },
        };
        let bytes = spec.to_bytes();
        assert_eq!(CellCreate::from_bytes(&bytes), Some(spec));
        for (offset, value) in [(16, 1), (24, 2), (48, 0), (88, 2)] {
            let mut bad = bytes;
            bad[offset] = value;
            assert!(CellCreate::from_bytes(&bad).is_none());
        }
        assert!(CellCreate::from_bytes(&bytes[..95]).is_none());
    }
    let spec = CellCreate::Slot(Allocation::new(4096, 4096).unwrap());
    let mut bytes = spec.to_bytes();
    assert_eq!(CellCreate::from_bytes(&bytes), Some(spec));
    bytes[95] = 1;
    assert!(CellCreate::from_bytes(&bytes).is_none());
}
#[test]
fn observation_preserves_generation_and_full_width_faults() {
    let info = CellInfo {
        level: CellLevel::BASE,
        generation: u64::MAX,
        state: CellState::Retracting,
        page: 7,
        frozen: false,
        external: true,
        address: VirtAddr::new(0x8000),
        fault_cause: u64::MAX,
        fault_address: u64::MAX,
    };
    let bytes = info.to_bytes();
    assert_eq!(CellInfo::from_bytes(&bytes), Some(info));
    for (offset, value) in [(8, 9), (24, 4), (56, 255)] {
        let mut bad = bytes;
        bad[offset] = value;
        assert!(CellInfo::from_bytes(&bad).is_none());
    }
}

#[test]
fn external_destination_rejects_internal_only_controls_and_geometry() {
    use kernel_api::model::modifiable::Modifiable;
    let request = CellCreate::Memo {
        source: CellSource::Slot(ReferenceRange {
            reference: SlotId::from_bytes(&3u64.to_le_bytes()).unwrap(),
            offset: 0,
            count: 1,
        }),
        destination: MemoDestination::External {
            authority: PendId::mint(4),
            address: VirtAddr::new(0x8000),
            access: Modifiable::No,
        },
    };
    let bytes = request.to_bytes();
    assert_eq!(CellCreate::from_bytes(&bytes), Some(request));
    for (word, value) in [(5, 1usize), (7, 0), (8, 0x8001), (10, 10), (11, 1)] {
        let mut bad = bytes;
        bad[word * 8..word * 8 + 8].copy_from_slice(&value.to_le_bytes());
        assert!(CellCreate::from_bytes(&bad).is_none(), "word {word}");
    }
}
#[test]
fn creation_reply_rejects_unknown_kinds_ids_and_cross_kind_payloads() {
    assert!(matches!(
        CreatedCell::decode(0, 1, 0),
        Some(CreatedCell::Slot(_))
    ));
    assert!(matches!(
        CreatedCell::decode(1, 2, 4096),
        Some(CreatedCell::Memo { .. })
    ));
    for (kind, id, address) in [
        (2, 1, 0),
        (0, 0, 0),
        (1, usize::MAX, 0),
        (0, 1, 4096),
        (1, 1, 1),
    ] {
        assert!(CreatedCell::decode(kind, id, address).is_none());
    }
}
