use kernel_api::boot::ledger::Entries;
use kernel_api::boot::ledger::Entry;
use kernel_api::boot::ledger::Header;
use kernel_api::boot::ledger::Name;
use kernel_api::boot::ledger::Page;
use kernel_api::boot::ledger::ResourceKind;
use kernel_api::boot::ledger::VERSION;
use kernel_api::boot::ledger::store_entries;

fn entry() -> Entry {
    Entry::new(Name::Page(Page::Dtb), ResourceKind::Share, 19)
}

fn ledger(entries: &[Entry]) -> Vec<u8> {
    let mut bytes = vec![0xaa; Header::LEN + entries.len() * kernel_api::boot::ledger::ENTRY_LEN];
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
fn ledger_keeps_header_entry_offsets_and_reserved_bytes() {
    let bytes = ledger(&[entry()]);
    let mut expected = vec![0u8; 48];
    expected[..4].copy_from_slice(&VERSION.to_le_bytes());
    expected[4] = 1;
    expected[8] = 2;
    expected[32] = 19;
    assert_eq!(bytes, expected);
    assert_eq!(kernel_api::boot::ledger::ENTRY_LEN, 40);
    assert_eq!(Header::LEN, 8);
    let entries = Entries::new(&bytes).unwrap();
    assert_eq!(entries.iter().collect::<Vec<_>>(), vec![entry()]);
    assert_eq!(entries.find(Name::Page(Page::Dtb)), Some(entry()));
    let mut out = [0xaa; 48];
    assert_eq!(entry().store_at(&mut out, 4), Some(44));
    assert_eq!(&out[..4], &[0xaa; 4]);
    assert_eq!(&out[44..], &[0xaa; 4]);
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
            ResourceKind::Clone,
            19
        )]))
        .is_none()
    );
    for (offset, value) in [(0, 1), (4, 255), (8, 255), (10, 1), (24, 255), (25, 1)] {
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
            .handle(),
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
        Entry::new(Name::Page(Page::Initrd), ResourceKind::Share, 23),
    ];
    let expected = ledger(&values);
    let mut out = [0xaa; 128];
    let len = store_entries(&mut out, &values).unwrap();
    assert_eq!(len, 88);
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

#[test]
fn boot_creation_license_sign_and_nest_are_distinct_pend_targets() {
    use kernel_api::boot::ledger::{Call, Trap};
    let values = [
        Entry::new(Name::Call(Call::Create), ResourceKind::None, 31),
        Entry::new(Name::Trap(Trap::SupervisorExternal), ResourceKind::Sign, 32),
        Entry::new(
            Name::Nest { controller: 0x30000000, requester: 0 },
            ResourceKind::Nest,
            33,
        ),
    ];
    let bytes = ledger(&values);
    assert_eq!(
        Entries::new(&bytes).unwrap().iter().collect::<Vec<_>>(),
        values
    );
    let mut stale = bytes.clone();
    stale[..4].copy_from_slice(&(VERSION - 1).to_le_bytes());
    assert!(Entries::new(&stale).is_none());
    let wrong = ledger(&[Entry::new(
        Name::Call(Call::Create),
        ResourceKind::Control,
        31,
    )]);
    assert!(Entries::new(&wrong).is_none());
}

#[test]
fn trusted_nest_routes_are_descriptions_not_port_handles() {
    let name = Name::Nest { controller: 0x30000000, requester: 8 };
    let record = Entry::new(name, ResourceKind::Nest, 101);
    let bytes = ledger(&[record]);
    assert_eq!(Entries::new(&bytes).unwrap().find(name), Some(record));
    assert_eq!(bytes[8], 5);
    assert_eq!(bytes[12], 8);
    assert_eq!(bytes[24], 7);
    for raw in [0, u32::MAX] {
        let record = Entry::new(
            Name::Nest { controller: 0x30000000, requester: raw },
            ResourceKind::Nest,
            101,
        );
        // The codec does not authenticate topology; trusted admission does.
        assert!(Entries::new(&ledger(&[record])).is_some());
    }
    assert!(Entries::new(&ledger(&[Entry::new(
        Name::Nest { controller: 0, requester: 8 },
        ResourceKind::Nest,
        101,
    )])).is_none());
    assert!(Entries::new(&ledger(&[Entry::new(name, ResourceKind::Share, 101)])).is_none());
    let mut old_kind = bytes.clone();
    old_kind[24] = 6;
    assert!(Entries::new(&old_kind).is_none());
    let mut old_name = bytes;
    old_name[8] = 3;
    old_name[12..16].fill(0);
    assert!(Entries::new(&old_name).is_none());
}

#[test]
fn dispatch_pairs_require_a_typed_view_and_sign() {
    use kernel_api::model::view::ViewKind;
    let name = Name::Interrupt {
        controller: 0xc000000,
        source: 10,
    };
    let pair = Entry::pair(name, ViewKind::Share, 42, 43);
    let bytes = ledger(&[pair]);
    assert_eq!(
        Entries::new(&bytes).unwrap().find(name).unwrap().dispatch(),
        Some(((ViewKind::Share, 42), 43))
    );
    for invalid in [
        Entry::new(name, ResourceKind::Sign, 43),
        Entry::pair(name, ViewKind::Share, 0, 43),
        Entry::pair(name, ViewKind::Share, 43, 43),
        Entry::pair(name, ViewKind::Clone, 42, 43),
    ] {
        assert!(Entries::new(&ledger(&[invalid])).is_none());
    }
    for at in [25, 26] {
        let mut bad = bytes.clone();
        bad[at] = 255;
        assert!(Entries::new(&bad).is_none());
    }
}
