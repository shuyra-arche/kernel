use kernel_api::address::VirtAddr;
use kernel_api::model::cell::Placement;
use kernel_api::model::cell::ReferenceRange;
use kernel_api::model::cell::SlotId;

fn reference() -> SlotId {
    SlotId::from_bytes(&17u64.to_le_bytes()).unwrap()
}

#[test]
fn memory_descriptors_keep_their_exact_little_endian_layout() {
    let range = ReferenceRange::new(reference(), 3, 5).unwrap();
    let mut bytes = [0xaa; ReferenceRange::WIDTH];
    range.store(&mut bytes);
    let expected: Vec<u8> = [17u64, 3, 5]
        .into_iter()
        .flat_map(u64::to_le_bytes)
        .collect();
    assert_eq!(bytes.as_slice(), expected);
    assert_eq!(ReferenceRange::fetch(&bytes), Some(range));

    let placement = Placement::exact(VirtAddr::new(0x8000), 0x2000).unwrap();
    let mut bytes = [0xaa; Placement::WIDTH];
    placement.store(&mut bytes);
    let expected: Vec<u8> = [2u64, 0x8000, 0x2000]
        .into_iter()
        .flat_map(u64::to_le_bytes)
        .collect();
    assert_eq!(bytes.as_slice(), expected);
    assert_eq!(Placement::fetch(&bytes), Some(placement));
}

#[test]
fn malformed_memory_descriptors_are_rejected() {
    assert!(ReferenceRange::fetch(&[0; 23]).is_none());
    assert!(ReferenceRange::fetch(&[0; 25]).is_none());
    assert!(ReferenceRange::fetch(&[0; 24]).is_none());
    for words in [[1u64, u64::MAX, 1], [u64::MAX, 0, 1]] {
        let bytes: Vec<_> = words.into_iter().flat_map(u64::to_le_bytes).collect();
        assert!(ReferenceRange::fetch(&bytes).is_none());
    }
    for words in [[3u64, 0, 4096], [0, 1, 4096], [0, 0, 3], [2, 1, 4096]] {
        let bytes: Vec<_> = words.into_iter().flat_map(u64::to_le_bytes).collect();
        assert!(Placement::fetch(&bytes).is_none());
    }
    assert!(Placement::fetch(&[0; 23]).is_none());
    assert!(Placement::fetch(&[0; 25]).is_none());
}
