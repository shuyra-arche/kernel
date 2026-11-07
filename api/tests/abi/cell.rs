use kernel_api::{
    address::VirtAddr,
    model::cell::{
        Attachment, FlatReference, MappingControl, MappingId, Placement, Position, ReferenceRange,
    },
    model::identity::TeamId,
};

#[test]
fn position_rejects_zero_identity_unknown_kind_and_nonzero_reserved_word() {
    let reference = FlatReference::from_bytes(&1u64.to_le_bytes()).unwrap();
    let mapping = MappingId::from_bytes(&7u64.to_le_bytes()).unwrap();
    for position in [
        Position::Slot { reference, page: 3 },
        Position::Memo {
            team: TeamId::new(0),
            mapping,
            page: 5,
        },
    ] {
        assert_eq!(Position::from_bytes(&position.to_bytes()), Some(position));
    }
    let mut bytes = Position::Slot { reference, page: 3 }.to_bytes();
    bytes[24] = 1;
    assert!(Position::from_bytes(&bytes).is_none());
    bytes[24] = 0;
    bytes[8] = 0;
    assert!(Position::from_bytes(&bytes).is_none());
    bytes[0] = 2;
    assert!(Position::from_bytes(&bytes).is_none());
    assert!(Position::from_bytes(&[0; 31]).is_none());
}

#[test]
fn attachment_has_one_checked_snapshot_for_geometry_authority_and_receipt() {
    let reference = FlatReference::from_bytes(&3u64.to_le_bytes()).unwrap();
    let spec = Attachment {
        range: ReferenceRange {
            reference,
            offset: 0,
            count: 1,
        },
        placement: Placement::exact(VirtAddr::new(0x8000), 4096).unwrap(),
        access: 6,
        control: MappingControl::Caller,
        change: 29,
    };
    let bytes = spec.to_bytes();
    assert_eq!(Attachment::from_bytes(&bytes), Some(spec));
    let mut bad = bytes;
    bad[56] = 2;
    assert!(Attachment::from_bytes(&bad).is_none());
    let mut bad = bytes;
    bad[16..24].fill(0);
    assert!(Attachment::from_bytes(&bad).is_none());
    assert!(Attachment::from_bytes(&bytes[..71]).is_none());
}
