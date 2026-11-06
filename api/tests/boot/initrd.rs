use kernel_api::boot::initrd::{PREAMBLE, entry_image};

#[test]
fn preamble_selects_only_the_initial_capsule() {
    let mut blob = Vec::new();
    blob.extend_from_slice(&(PREAMBLE as u32 + 4).to_le_bytes());
    blob.extend_from_slice(&3u32.to_le_bytes());
    blob.extend_from_slice(b"metaELFtrailing");
    assert_eq!(entry_image(&blob), Some(b"ELF".as_slice()));
}

#[test]
fn preamble_rejects_truncation_and_out_of_bounds_spans() {
    for len in 0..PREAMBLE {
        assert!(entry_image(&vec![0; len]).is_none());
    }
    for (offset, length) in [(8u32, 1u32), (9, 0), (u32::MAX, u32::MAX)] {
        let mut blob = offset.to_le_bytes().to_vec();
        blob.extend_from_slice(&length.to_le_bytes());
        assert!(entry_image(&blob).is_none());
    }
}
