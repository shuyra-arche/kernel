//! Domain-local request codecs and the shared wire header.
use kernel_api::{
    abi::memory::{Domain, RequestId, VERSION, flat},
    address::PageCount,
    model::{
        cell::CellLevel,
        flat::{FlatId, FlatVersion, SlotPosition},
        identity::PendId,
        nest::NestVersion,
    },
};

fn word(bytes: &[u8], index: usize) -> u64 {
    u64::from_le_bytes(bytes[index * 8..index * 8 + 8].try_into().unwrap())
}

#[test]
fn flat_delete_uses_token_and_version_only() {
    let action = flat::Action::Delete {
        token: PendId::mint(3),
        expected: FlatVersion::new(2).unwrap(),
    };
    let request = flat::Request::new(RequestId::new(7), action).unwrap();
    let bytes = request.to_bytes();
    assert_eq!(word(&bytes, 0), VERSION);
    assert_eq!(word(&bytes, 1), Domain::Flat as u64);
    assert_eq!(word(&bytes, 2), flat::Opcode::Delete.slot());
    assert_eq!(word(&bytes, 3), 7);
    assert_eq!(word(&bytes, 4), 3);
    assert_eq!(word(&bytes, 5), 2);
    assert_eq!(flat::Request::from_bytes(&bytes).unwrap(), request);
}

#[test]
fn flat_create_roundtrips_page_count() {
    let action = flat::Action::Create {
        license: PendId::mint(3),
        count: PageCount::new(9).unwrap(),
        level: CellLevel::BASE,
    };
    let request = flat::Request::new(RequestId::new(8), action).unwrap();
    let bytes = request.to_bytes();
    assert_eq!(word(&bytes, 5), 9);
    assert_eq!(flat::Request::from_bytes(&bytes).unwrap(), request);
}

#[test]
fn flat_inspection_requires_no_request_id() {
    let action = flat::Action::try_from(kernel_api::abi::memory::Action::Inspect(
        kernel_api::abi::memory::Query::Slot(
            SlotPosition::new(PendId::mint(3), FlatId::new(11).unwrap(), 4).unwrap(),
        ),
    ))
    .unwrap();
    assert!(flat::Request::new(RequestId::new(9), action).is_none());
    assert!(flat::Request::new(None, action).is_some());
}
