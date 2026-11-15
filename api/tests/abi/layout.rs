use kernel_api::address::VirtAddr;
use kernel_api::model::cell::Placement;

#[test]
fn memory_descriptors_keep_their_exact_little_endian_layout() {
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
    for words in [[3u64, 0, 4096], [0, 1, 4096], [0, 0, 3], [2, 1, 4096]] {
        let bytes: Vec<_> = words.into_iter().flat_map(u64::to_le_bytes).collect();
        assert!(Placement::fetch(&bytes).is_none());
    }
    assert!(Placement::fetch(&[0; 23]).is_none());
    assert!(Placement::fetch(&[0; 25]).is_none());
}
