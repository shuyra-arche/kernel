use kernel_api::boot::capsule::Capsule;
use kernel_api::boot::capsule::HEADER;
use kernel_api::boot::capsule::MAGIC;
use kernel_api::boot::capsule::PAGE;
use kernel_api::boot::capsule::RECORD;

fn number(bytes: &mut [u8], at: usize, value: u64, width: usize) {
    bytes[at..at + width].copy_from_slice(&value.to_le_bytes()[..width]);
}

fn capsule() -> Vec<u8> {
    let mut bytes = vec![0; PAGE * 2];
    bytes[..8].copy_from_slice(&MAGIC);
    number(&mut bytes, 8, 2, 2);
    bytes[10] = 12;
    bytes[11] = kernel_api::abi::VERSION as u8;
    number(&mut bytes, 12, 1, 4);
    number(&mut bytes, 16, 0x1000, 8);
    number(&mut bytes, 24, (PAGE * 2) as u64, 8);
    number(&mut bytes, HEADER, 0x1000, 8);
    number(&mut bytes, HEADER + 8, 1, 8);
    number(&mut bytes, HEADER + 16, 1, 8);
    number(&mut bytes, HEADER + 24, PAGE as u64, 8);
    number(&mut bytes, HEADER + 32, 5, 4); // read + execute
    bytes
}

#[test]
fn capsule_preserves_entry_and_region_layout() {
    let bytes = capsule();
    let image = Capsule::parse(&bytes).unwrap();
    assert_eq!(kernel_api::boot::capsule::VERSION, 2);
    assert_eq!(bytes[11], 10);
    assert_eq!(image.entry, 0x1000);
    assert_eq!(image.count, 1);
    let region = image.region(0).unwrap();
    assert_eq!((region.va, region.pages, region.data_pages), (0x1000, 1, 1));
    assert_eq!((region.payload, region.flags), (PAGE, 10));
    assert!(image.region(1).is_none());
    for version in
        (0..kernel_api::abi::VERSION as u8).chain([kernel_api::abi::VERSION as u8 + 1, u8::MAX])
    {
        let mut invalid = bytes.clone();
        invalid[11] = version;
        assert!(Capsule::parse(&invalid).is_none(), "ABI {version}");
    }
}

#[test]
fn capsule_rejects_invalid_execute_permissions_ranges_and_payloads() {
    for (offset, value, width) in [
        (8, 1, 2),                                      // retired capsule format
        (11, 0, 1),                                     // retired kernel ABI
        (11, (kernel_api::abi::VERSION + 1) as u64, 1), // future kernel ABI
        (11, 1, 1),                                     // retired descriptor layout
        (16, 0x3000, 8),                                // entry outside executable region
        (HEADER + 32, 7, 4),                            // writable executable region
        (HEADER + 36, 1, 4),                            // nonzero reserved field
        (HEADER + 8, u64::MAX, 8),                      // overflowing virtual span
        (HEADER + 16, 2, 8),                            // data exceeds mapped pages
        (HEADER + 24, (PAGE * 2) as u64, 8),            // payload outside image
        (HEADER + 24, HEADER as u64, 8),                // payload points into header
    ] {
        let mut bytes = capsule();
        number(&mut bytes, offset, value, width);
        assert!(
            Capsule::parse(&bytes).is_none(),
            "offset {offset}, value {value}"
        );
    }
    let mut bytes = capsule();
    number(&mut bytes, 12, 2, 4);
    let first = bytes[HEADER..HEADER + RECORD].to_vec();
    bytes[HEADER + RECORD..HEADER + RECORD * 2].copy_from_slice(&first);
    assert!(Capsule::parse(&bytes).is_none()); // overlapping regions
}
