use env::ledger::entry::{Entries, Header, VERSION, store_entries};
use env::{
    Entry, FlatReference, Name, Page, PieKind, PieToken, Placement, ReferenceRange, VirtAddr,
};
use kernel_api_env as env;

fn reference() -> FlatReference {
    FlatReference::from_bytes(&17u64.to_le_bytes()).unwrap()
}
fn entry() -> Entry {
    Entry::new(Name::Page(Page::Dtb), PieKind::Pole, PieToken::mint(19))
}

fn ledger(entries: &[Entry]) -> Vec<u8> {
    let mut bytes = vec![0xaa; Header::LEN + entries.len() * env::ENTRY_LEN];
    let mut at = Header {
        version: VERSION,
        count: entries.len() as u32,
    }
    .store_at(&mut bytes, 0)
    .unwrap();
    for value in entries {
        at = value.store_at(&mut bytes, at).unwrap();
    }
    assert_eq!(at, bytes.len());
    bytes
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

#[test]
fn ledger_keeps_header_entry_offsets_and_reserved_bytes() {
    let bytes = ledger(&[entry()]);
    let mut expected = vec![0u8; 40];
    expected[0] = 1; // version
    expected[4] = 1; // count
    expected[8] = 2; // Name::Page class, Dtb item 0
    expected[32] = 19; // token, after name/kind/seven reserved bytes
    assert_eq!(bytes, expected);
    assert_eq!(env::ENTRY_LEN, 32);
    assert_eq!(Header::LEN, 8);
    let entries = Entries::new(&bytes).unwrap();
    assert_eq!(entries.iter().collect::<Vec<_>>(), vec![entry()]);
    assert_eq!(entries.find(Name::Page(Page::Dtb)), Some(entry()));
    let mut out = [0xaa; 40];
    assert_eq!(entry().store_at(&mut out, 4), Some(36));
    assert_eq!(&out[..4], &[0xaa; 4]);
    assert_eq!(&out[36..], &[0xaa; 4]);
}

#[test]
fn ledger_rejects_truncation_trailing_data_duplicates_and_invalid_fields() {
    let bytes = ledger(&[entry()]);
    for end in 0..bytes.len() {
        assert!(Entries::new(&bytes[..end]).is_none());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(Entries::new(&trailing).is_none());
    assert!(Entries::new(&ledger(&[entry(), entry()])).is_none());
    assert!(Entries::new(&ledger(&[Entry::NONE])).is_none());
    assert!(
        Entries::new(&ledger(&[Entry::new(
            Name::Page(Page::Dtb),
            PieKind::Hole,
            PieToken::mint(19)
        )]))
        .is_none()
    );
    for (offset, value) in [(0, 2), (4, 255), (8, 255), (10, 1), (24, 255), (25, 1)] {
        let mut malformed = bytes.clone();
        malformed[offset] = value;
        assert!(Entries::new(&malformed).is_none(), "offset {offset}");
    }
    let mut high_token = bytes.clone();
    high_token[39] = 128;
    assert_eq!(
        Entries::new(&high_token)
            .unwrap()
            .iter()
            .next()
            .unwrap()
            .token()
            .get(),
        (1usize << 63) | 19
    );
    assert_eq!(Entries::new(&ledger(&[])).unwrap().iter().count(), 0);
}

#[test]
fn fixed_codecs_reject_overflowing_offsets_and_short_buffers() {
    assert!(Entry::fetch_at(&[0; 32], usize::MAX).is_none());
    assert!(Header::fetch_at(&[0; 8], usize::MAX).is_none());
    assert!(entry().store_at(&mut [0; 32], usize::MAX).is_none());
    assert!(entry().store_at(&mut [0; 31], 0).is_none());
    assert!(
        Header {
            version: VERSION,
            count: 0
        }
        .store_at(&mut [0; 7], 0)
        .is_none()
    );
}

#[test]
fn ledger_writer_distinguishes_used_length_from_buffer_capacity() {
    let values = [
        entry(),
        Entry::new(Name::Page(Page::Initrd), PieKind::Pole, PieToken::mint(23)),
    ];
    let expected = ledger(&values);
    let mut out = [0xaa; 128];
    let len = store_entries(&mut out, &values).unwrap();
    assert_eq!(len, 72);
    assert_eq!(&out[..len], expected);
    assert!(out[len..].iter().all(|byte| *byte == 0xaa));
    assert_eq!(
        Entries::new(&out[..len])
            .unwrap()
            .iter()
            .collect::<Vec<_>>(),
        values
    );
    assert!(Entries::new(&out).is_none());
    for capacity in 0..len {
        let mut short = vec![0xaa; capacity];
        assert_eq!(store_entries(&mut short, &values), None);
        assert!(short.iter().all(|byte| *byte == 0xaa));
    }
    assert_eq!(store_entries(&mut out, &[]), Some(Header::LEN));
    assert_eq!(Entries::new(&out[..Header::LEN]).unwrap().iter().count(), 0);
}
