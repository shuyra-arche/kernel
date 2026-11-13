use super::{memo, slot};
use kernel_api::{
    abi::memory::{Action, Domain, Maintenance, Opcode, Request, RequestId, Source},
    model::{
        flat::{BlockGeneration, BlockRef, FlatId, SlotPosition, SlotVersion},
        identity::PendId,
        nest::MemoGeneration,
        view::ViewId,
    },
};

pub(super) fn id() -> RequestId {
    RequestId::new(u64::MAX).unwrap()
}

pub(super) fn maintenance() -> Maintenance {
    Maintenance {
        token: PendId::mint(23),
        block: BlockRef::new(
            FlatId::new(11).unwrap(),
            29,
            BlockGeneration::new(31).unwrap(),
        )
        .unwrap(),
    }
}

fn sources() -> [Source; 3] {
    [
        Source::Flat {
            position: slot(512),
            expected: SlotVersion::new(1 << 63).unwrap(),
        },
        Source::Share {
            token: PendId::mint(37),
            view: ViewId::new(41),
            page: 512,
            count: 512,
        },
        Source::Memo {
            position: memo(512),
            expected: MemoGeneration::new(1 << 63).unwrap(),
        },
    ]
}

fn mutations() -> Vec<Action> {
    let slot = slot(512);
    let memo = memo(512);
    let sv = SlotVersion::new(43).unwrap();
    let mg = MemoGeneration::new(47).unwrap();
    let maintenance = maintenance();
    let mut actions = vec![
        Action::Occupy { slot, expected: sv },
        Action::Vacate {
            slot,
            expected: sv,
            maintenance,
        },
        Action::Block {
            start: slot,
            count: 512,
            recover: None,
        },
        Action::Block {
            start: slot,
            count: 512,
            recover: Some(maintenance.block),
        },
        Action::Allow { maintenance },
        Action::Remove { memo, expected: mg },
        Action::Retract { memo, expected: mg },
        Action::Protect {
            memo,
            expected: mg,
            access: 6,
        },
    ];
    for maintenance in [None, Some(maintenance)] {
        actions.push(Action::Push {
            local: slot,
            local_version: sv,
            remote: super::slot(1024),
            remote_version: sv,
            maintenance,
        });
        actions.push(Action::Pull {
            local: slot,
            local_version: sv,
            remote: super::slot(1024),
            remote_version: sv,
            maintenance,
        });
        actions.push(Action::Publish {
            memo,
            expected: mg,
            maintenance,
        });
    }
    for source in sources() {
        actions.push(Action::Insert {
            target: memo,
            expected: mg,
            source,
        });
    }
    actions.extend(super::objects::mutations());
    actions.extend(super::reshape::mutations());
    actions
}

pub(super) fn set_word(bytes: &mut [u8], index: usize, value: u64) {
    bytes[index * 8..index * 8 + 8].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn settled_operations_have_distinct_domain_and_slot_contracts() {
    for (opcode, domain, slot) in [
        (Opcode::Create(Domain::Flat), 12, 16),
        (Opcode::Delete(Domain::Flat), 12, 17),
        (Opcode::Create(Domain::Nest), 13, 16),
        (Opcode::Delete(Domain::Nest), 13, 17),
        (Opcode::Reserve, 13, 19),
        (Opcode::Release, 13, 20),
        (Opcode::Split, 12, 25),
        (Opcode::Merge, 12, 26),
        (Opcode::Inspect(Domain::Flat), 12, 18),
        (Opcode::Inspect(Domain::Nest), 13, 18),
        (Opcode::Occupy, 12, 19),
        (Opcode::Vacate, 12, 20),
        (Opcode::Block, 12, 21),
        (Opcode::Allow, 12, 22),
        (Opcode::Push, 12, 23),
        (Opcode::Pull, 12, 24),
        (Opcode::Insert, 13, 21),
        (Opcode::Remove, 13, 22),
        (Opcode::Publish, 13, 23),
        (Opcode::Retract, 13, 24),
        (Opcode::Protect, 13, 25),
    ] {
        assert_eq!((opcode.domain() as u64, opcode.slot()), (domain, slot));
        assert_eq!(Opcode::of(domain, slot), Some(opcode));
        let number = (11 << 48) | (domain << 32) | slot;
        assert_eq!(opcode.call_number(), number);
        assert_eq!(Opcode::from_call_number(number), Some(opcode));
    }
    for (domain, slot) in [
        (2, 0),
        (12, 0),
        (12, 27),
        (13, 26),
        (13, 27),
        (14, 21),
        (12, u64::MAX),
        (u64::MAX, 21),
    ] {
        assert!(Opcode::of(domain, slot).is_none());
    }
}

#[test]
fn request_vectors_keep_fixed_positions_versions_and_full_width_identity() {
    let request = Request::new(
        Some(id()),
        Action::Occupy {
            slot: slot(512),
            expected: SlotVersion::new(1 << 63).unwrap(),
        },
    )
    .unwrap();
    let mut expected = [0u8; 256];
    for (index, word) in [11, 12, 19, u64::MAX, 7, 11, 512, 1 << 63]
        .into_iter()
        .enumerate()
    {
        set_word(&mut expected, index, word);
    }
    assert_eq!(request.to_bytes(), expected);
    assert_eq!(Request::from_bytes(&expected), Some(request));
    assert_eq!(request.id(), Some(id()));
    assert!(id().next().is_none());
    assert!(RequestId::new(0).is_none());
    assert_eq!(RequestId::new(1).unwrap().next().unwrap().get(), 2);

    for action in mutations() {
        let request = Request::new(Some(id()), action).unwrap();
        assert_eq!(Request::from_bytes(&request.to_bytes()), Some(request));
        assert_eq!(request.action(), action);
        assert!(Request::new(None, action).is_none());
    }
    for domain in [Domain::Flat, Domain::Nest] {
        let action = Action::InspectResult {
            domain,
            request: id(),
        };
        assert!(Request::new(Some(id()), action).is_none());
        let query = Request::new(None, action).unwrap();
        let mut expected = [0u8; 256];
        for (index, word) in [11, domain as u64, 18, 0, 3, u64::MAX]
            .into_iter()
            .enumerate()
        {
            set_word(&mut expected, index, word);
        }
        assert_eq!(query.to_bytes(), expected);
        assert_eq!(Request::from_bytes(&expected), Some(query));
    }
}

#[test]
fn malformed_headers_payloads_and_reserved_tails_reject() {
    let occupy = Request::new(
        Some(id()),
        Action::Occupy {
            slot: slot(512),
            expected: SlotVersion::INITIAL,
        },
    )
    .unwrap();
    for (word, invalid) in [
        (0, 10),
        (0, 12),
        (1, 2),
        (1, 13),
        (2, 0),
        (2, 26),
        (3, 0),
        (4, 0),
        (5, 0),
        (6, u64::MAX),
        (7, 0),
        (8, 1),
        (31, 1),
    ] {
        let mut bytes = occupy.to_bytes();
        set_word(&mut bytes, word, invalid);
        assert!(
            Request::from_bytes(&bytes).is_none(),
            "word {word}, {invalid}"
        );
    }
    for len in 0..Request::WIDTH {
        assert!(Request::from_bytes(&occupy.to_bytes()[..len]).is_none());
    }
    let mut longer = occupy.to_bytes().to_vec();
    longer.push(0);
    assert!(Request::from_bytes(&longer).is_none());

    let query = Request::new(
        None,
        Action::InspectResult {
            domain: Domain::Flat,
            request: id(),
        },
    )
    .unwrap();
    for (word, invalid) in [(3, 1), (4, 0), (4, 4), (5, 0), (6, 1)] {
        let mut bytes = query.to_bytes();
        set_word(&mut bytes, word, invalid);
        assert!(Request::from_bytes(&bytes).is_none());
    }
    let publish = Request::new(
        Some(id()),
        Action::Publish {
            memo: memo(512),
            expected: MemoGeneration::INITIAL,
            maintenance: None,
        },
    )
    .unwrap();
    for (word, invalid) in [(9, 2), (10, 1)] {
        let mut bytes = publish.to_bytes();
        set_word(&mut bytes, word, invalid);
        assert!(Request::from_bytes(&bytes).is_none());
    }
    let insert = Request::new(
        Some(id()),
        Action::Insert {
            target: memo(512),
            expected: MemoGeneration::INITIAL,
            source: sources()[1],
        },
    )
    .unwrap();
    for (word, invalid) in [(9, 3), (10, 0), (11, 0), (12, u64::MAX), (13, 0)] {
        let mut bytes = insert.to_bytes();
        set_word(&mut bytes, word, invalid);
        assert!(Request::from_bytes(&bytes).is_none());
    }
}

#[test]
fn source_range_access_and_recovery_checks_are_shared_by_construction_and_decode() {
    for (page, count) in [(0, 0), (usize::MAX, 1), (0, usize::MAX)] {
        let source = Source::Share {
            token: PendId::mint(1),
            view: ViewId::new(1),
            page,
            count,
        };
        assert!(
            Request::new(
                Some(id()),
                Action::Insert {
                    target: memo(0),
                    expected: MemoGeneration::INITIAL,
                    source
                }
            )
            .is_none()
        );
    }
    for access in [0, 1, 4, 12, 16, u64::MAX] {
        assert!(
            Request::new(
                Some(id()),
                Action::Protect {
                    memo: memo(0),
                    expected: MemoGeneration::INITIAL,
                    access
                }
            )
            .is_none()
        );
    }
    for access in [2, 6, 8, 10, 14] {
        let request = Request::new(
            Some(id()),
            Action::Protect {
                memo: memo(0),
                expected: MemoGeneration::INITIAL,
                access,
            },
        )
        .unwrap();
        assert_eq!(Request::from_bytes(&request.to_bytes()), Some(request));
    }
    assert!(
        Request::new(
            Some(id()),
            Action::Block {
                start: slot(0),
                count: 0,
                recover: None
            }
        )
        .is_none()
    );
    let wrong_flat = BlockRef::new(FlatId::new(99).unwrap(), 1, BlockGeneration::INITIAL).unwrap();
    assert!(
        Request::new(
            Some(id()),
            Action::Block {
                start: slot(0),
                count: 1,
                recover: Some(wrong_flat)
            }
        )
        .is_none()
    );
    let alias = SlotPosition::new(PendId::mint(99), slot(0).flat(), 0).unwrap();
    assert!(
        Request::new(
            Some(id()),
            Action::Push {
                local: slot(0),
                local_version: SlotVersion::INITIAL,
                remote: alias,
                remote_version: SlotVersion::INITIAL,
                maintenance: None
            }
        )
        .is_none()
    );
    let invalid = Maintenance {
        token: PendId::NONE,
        ..maintenance()
    };
    assert!(
        Request::new(
            Some(id()),
            Action::Allow {
                maintenance: invalid
            }
        )
        .is_none()
    );
    // A staged Slot may be in a different Flat from the maintenance record.
    // Trace and original authority must be checked by Server, not guessed here.
    let staged = SlotPosition::new(PendId::mint(99), FlatId::new(101).unwrap(), 0).unwrap();
    assert!(
        Request::new(
            Some(id()),
            Action::Vacate {
                slot: staged,
                expected: SlotVersion::INITIAL,
                maintenance: maintenance()
            }
        )
        .is_some()
    );
}

#[test]
fn every_accepted_mutated_descriptor_has_one_canonical_encoding() {
    for action in mutations() {
        let original = Request::new(Some(id()), action).unwrap().to_bytes();
        for index in 0..original.len() {
            let mut bytes = original;
            bytes[index] ^= 0xff;
            if let Some(decoded) = Request::from_bytes(&bytes) {
                assert_eq!(
                    decoded.to_bytes(),
                    bytes,
                    "noncanonical byte {index} in {action:?}"
                );
            }
        }
    }
}

#[test]
fn trap_identity_must_match_the_snapshotted_request() {
    for action in mutations() {
        let request = Request::new(Some(id()), action).unwrap();
        let number = action.opcode().call_number();
        assert_eq!(
            Request::from_call(number, &request.to_bytes()),
            Some(request)
        );
        for wrong in [
            number ^ (1 << 48),
            number ^ (1 << 32),
            number ^ 1,
            number | (1 << 31),
        ] {
            assert!(Request::from_call(wrong, &request.to_bytes()).is_none());
        }
    }
}
