//! Domain-local request codecs and the shared wire header.
use kernel_api::{
    abi::memory::{Domain, RequestId, VERSION, flat, nest},
    address::{PageCount, PageOffset},
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

#[test]
fn nest_delete_uses_token_and_version_only() {
    let action = nest::Action::Delete {
        token: PendId::mint(3),
        expected: NestVersion::new(2).unwrap(),
    };
    let request = nest::Request::new(RequestId::new(11), action).unwrap();
    let bytes = request.to_bytes();
    assert_eq!(word(&bytes, 0), VERSION);
    assert_eq!(word(&bytes, 1), Domain::Nest as u64);
    assert_eq!(word(&bytes, 2), nest::Opcode::Delete.slot());
    assert_eq!(word(&bytes, 3), 11);
    assert_eq!(word(&bytes, 4), 3);
    assert_eq!(word(&bytes, 5), 2);
    assert_eq!(nest::Request::from_bytes(&bytes).unwrap(), request);
}

#[test]
fn nest_create_roundtrips_license() {
    let action = nest::Action::Create {
        license: PendId::mint(3),
    };
    let request = nest::Request::new(RequestId::new(12), action).unwrap();
    assert_eq!(
        nest::Request::from_bytes(&request.to_bytes()).unwrap(),
        request
    );
}

#[test]
fn nest_inspection_requires_no_request_id() {
    let action = nest::Action::try_from(kernel_api::abi::memory::Action::Inspect(
        kernel_api::abi::memory::Query::Nest {
            token: PendId::mint(3),
            nest: kernel_api::model::nest::NestId::new(11).unwrap(),
        },
    ))
    .unwrap();
    assert!(nest::Request::new(RequestId::new(13), action).is_none());
    assert!(nest::Request::new(None, action).is_some());
}

#[test]
fn domain_headers_reject_the_other_domain() {
    let flat = flat::Action::Delete {
        token: PendId::mint(3),
        expected: FlatVersion::new(2).unwrap(),
    };
    let bytes = flat::Request::new(RequestId::new(14), flat)
        .unwrap()
        .to_bytes();
    assert!(nest::Request::from_bytes(&bytes).is_none());
}

#[test]
fn flat_from_call_checks_the_trap_slot() {
    let action = flat::Action::Delete {
        token: PendId::mint(3),
        expected: FlatVersion::new(2).unwrap(),
    };
    let request = flat::Request::new(RequestId::new(15), action).unwrap();
    let bytes = request.to_bytes();
    let number = kernel_api::abi::memory::Opcode::Flat(flat::Opcode::Delete).call_number();
    assert_eq!(flat::Request::from_call(number, &bytes).unwrap(), request);
    let create = kernel_api::abi::memory::Opcode::Flat(flat::Opcode::Create).call_number();
    assert!(flat::Request::from_call(create, &bytes).is_none());
}

#[test]
fn range_slice_roundtrips_pend_coordinates() {
    let range = kernel_api::model::nest::RangeSlice::new(
        PendId::mint(5),
        PageOffset::new(7),
        PageCount::new(9).unwrap(),
    )
    .unwrap();
    assert_eq!(
        kernel_api::model::nest::RangeSlice::from_bytes(&range.to_bytes()),
        Some(range)
    );
    assert_eq!(range.range(), PendId::mint(5));
    assert_eq!(range.offset(), PageOffset::new(7));
    assert_eq!(range.count(), PageCount::new(9).unwrap());
}

#[test]
fn range_version_rejects_zero() {
    assert!(kernel_api::model::nest::RangeVersion::new(0).is_none());
    assert_eq!(
        kernel_api::model::nest::RangeVersion::new(3)
            .unwrap()
            .next()
            .unwrap()
            .get(),
        4
    );
}

#[test]
fn range_slice_rejects_zero_binding_and_truncation() {
    assert!(
        kernel_api::model::nest::RangeSlice::new(
            PendId::NONE,
            PageOffset::new(0),
            PageCount::new(1).unwrap(),
        )
        .is_none()
    );
    assert!(kernel_api::model::nest::RangeSlice::from_bytes(&[0; 23]).is_none());
}
