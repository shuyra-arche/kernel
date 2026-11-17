use super::request::{id, set_word};
use super::{nest_id, slot};
use kernel_api::{
    abi::memory::{Action, Failure, Outcome, Reply, Request, Value},
    address::{PageCount, VirtAddr},
    model::{
        cell::{CellLevel, Placement},
        flat::{FlatPhase, FlatVersion},
        identity::PendId,
        nest::{CreateSpanRequest, NestPhase, NestVersion, SpanPhase, SpanVersion},
    },
};

pub(super) fn span() -> CreateSpanRequest {
    CreateSpanRequest {
        placement: Placement::exact(VirtAddr::new(0x8000), 4096).unwrap(),
        count: PageCount::new(512).unwrap(),
        level: CellLevel::new(1).unwrap(),
        access: 6,
    }
}

pub(super) fn mutations() -> Vec<Action> {
    vec![
        Action::CreateFlat {
            license: PendId::mint(7),
            count: 1024,
            level: CellLevel::new(1).unwrap(),
        },
        Action::DeleteFlat {
            token: PendId::mint(7),
            flat: slot(0).flat(),
            expected: FlatVersion::new(u64::MAX).unwrap(),
        },
        Action::CreateNest {
            license: PendId::mint(7),
        },
        Action::DeleteNest {
            token: PendId::mint(7),
            nest: nest_id(),
            expected: NestVersion::new(u64::MAX).unwrap(),
        },
        Action::CreateSpan {
            token: PendId::mint(7),
            nest: nest_id(),
            expected: NestVersion::new(u64::MAX).unwrap(),
            request: span(),
        },
        Action::DeleteSpan {
            range: PendId::mint(7),
            expected: kernel_api::model::nest::RangeVersion::new(u64::MAX).unwrap(),
        },
    ]
}

pub(super) fn results() -> Vec<Reply> {
    let f = FlatVersion::new(1 << 63).unwrap();
    let n = NestVersion::new(1 << 63).unwrap();
    let m = SpanVersion::new(1 << 63).unwrap();
    let flat = slot(0).flat();
    let nest = nest_id();
    let mut results = vec![
        Reply::new(
            id(),
            kernel_api::abi::memory::Opcode::Flat(kernel_api::abi::memory::flat::Opcode::Create),
            Outcome::Committed(Value::CreatedFlat {
                token: PendId::mint(5),
                flat,
                version: f,
            }),
        )
        .unwrap(),
        Reply::new(
            id(),
            kernel_api::abi::memory::Opcode::Nest(kernel_api::abi::memory::nest::Opcode::Create),
            Outcome::Committed(Value::CreatedNest {
                token: PendId::mint(5),
                nest,
                version: n,
            }),
        )
        .unwrap(),
        Reply::new(
            id(),
            kernel_api::abi::memory::Opcode::Nest(kernel_api::abi::memory::nest::Opcode::Reserve),
            Outcome::Committed(Value::Span {
                range: PendId::mint(5),
                nest,
                nest_version: n,
                version: m,
                address: VirtAddr::new(0x8000),
            }),
        )
        .unwrap(),
    ];
    for phase in [FlatPhase::Closing, FlatPhase::Closed, FlatPhase::Failed] {
        let value = Value::FlatLifecycle {
            flat,
            version: f,
            phase,
        };
        let outcome = match phase {
            FlatPhase::Closing => Outcome::Pending(value),
            FlatPhase::Closed => Outcome::Committed(value),
            _ => Outcome::Failed {
                failure: Failure::Hardware,
                retained: Some(value),
            },
        };
        results.push(
            Reply::new(
                id(),
                kernel_api::abi::memory::Opcode::Flat(
                    kernel_api::abi::memory::flat::Opcode::Delete,
                ),
                outcome,
            )
            .unwrap(),
        );
    }
    for phase in [NestPhase::Closing, NestPhase::Closed, NestPhase::Failed] {
        let value = Value::NestLifecycle {
            nest,
            version: n,
            phase,
        };
        let outcome = match phase {
            NestPhase::Closing => Outcome::Pending(value),
            NestPhase::Closed => Outcome::Committed(value),
            _ => Outcome::Failed {
                failure: Failure::Hardware,
                retained: Some(value),
            },
        };
        results.push(
            Reply::new(
                id(),
                kernel_api::abi::memory::Opcode::Nest(
                    kernel_api::abi::memory::nest::Opcode::Delete,
                ),
                outcome,
            )
            .unwrap(),
        );
    }
    for phase in [SpanPhase::Closing, SpanPhase::Closed, SpanPhase::Failed] {
        let value = Value::SpanLifecycle {
            range: PendId::mint(5),
            version: m,
            phase,
        };
        let outcome = match phase {
            SpanPhase::Closing => Outcome::Pending(value),
            SpanPhase::Closed => Outcome::Committed(value),
            _ => Outcome::Failed {
                failure: Failure::Hardware,
                retained: Some(value),
            },
        };
        results.push(
            Reply::new(
                id(),
                kernel_api::abi::memory::Opcode::Nest(
                    kernel_api::abi::memory::nest::Opcode::Release,
                ),
                outcome,
            )
            .unwrap(),
        );
    }
    results
}

#[test]
fn object_mutations_use_expected_object_versions_and_exact_wire_layouts() {
    let payloads: &[&[u64]] = &[
        &[14, 12, 16, u64::MAX, 7, 1024, 1],
        &[14, 12, 17, u64::MAX, 7, 11, u64::MAX],
        &[14, 13, 16, u64::MAX, 7],
        &[14, 13, 17, u64::MAX, 7, 17, u64::MAX],
        &[
            14,
            13,
            19,
            u64::MAX,
            7,
            17,
            u64::MAX,
            2,
            0x8000,
            4096,
            512,
            1,
            6,
        ],
        &[14, 13, 20, u64::MAX, 7, u64::MAX],
    ];
    for (action, words) in mutations().into_iter().zip(payloads) {
        let request = Request::new(Some(id()), action).unwrap();
        let mut bytes = [0; Request::WIDTH];
        for (i, word) in words.iter().enumerate() {
            set_word(&mut bytes, i, *word);
        }
        assert_eq!(request.to_bytes(), bytes);
        assert_eq!(
            Request::from_call(action.opcode().call_number(), &bytes),
            Some(request)
        );
        assert!(Request::new(None, action).is_none());
        let mut bad = bytes;
        set_word(&mut bad, 4, 0);
        assert!(Request::from_bytes(&bad).is_none());
        let mut bad = bytes;
        set_word(&mut bad, words.len(), 1);
        assert!(Request::from_bytes(&bad).is_none());
    }
}

#[test]
fn creation_span_and_deletion_reject_bad_geometry_identity_or_version() {
    for count in [0, 1, 513, usize::MAX] {
        assert!(
            Request::new(
                Some(id()),
                Action::CreateFlat {
                    license: PendId::mint(1),
                    count,
                    level: CellLevel::new(1).unwrap()
                }
            )
            .is_none()
        );
    }
    let reserve = mutations()[4];
    let bytes = Request::new(Some(id()), reserve).unwrap().to_bytes();
    for (word, value) in [
        (5, 0),
        (6, 0),
        (7, 3),
        (8, 0x8001),
        (9, 0),
        (10, 0),
        (10, 1),
        (11, 7),
        (12, 4),
        (13, 1),
    ] {
        let mut bad = bytes;
        set_word(&mut bad, word, value);
        assert!(Request::from_bytes(&bad).is_none(), "word {word}, {value}");
    }
    // The base may be representable while the full span wraps.
    let top = usize::MAX & !4095;
    let bad = CreateSpanRequest {
        placement: Placement::exact(VirtAddr::new(top), 4096).unwrap(),
        count: PageCount::new(1).unwrap(),
        level: CellLevel::BASE,
        ..span()
    };
    assert!(
        Request::new(
            Some(id()),
            Action::CreateSpan {
                token: PendId::mint(1),
                nest: nest_id(),
                expected: NestVersion::INITIAL,
                request: bad
            }
        )
        .is_none()
    );
    let mut bad_bytes = bytes;
    set_word(&mut bad_bytes, 8, top as u64);
    assert!(Request::from_bytes(&bad_bytes).is_none());
    assert!(
        Request::new(
            Some(id()),
            Action::DeleteSpan {
                range: PendId::NONE,
                expected: kernel_api::model::nest::RangeVersion::INITIAL,
            },
        )
        .is_none()
    );
    for index in [1, 3, 5] {
        let action = mutations()[index];
        let mut bad = Request::new(Some(id()), action).unwrap().to_bytes();
        set_word(&mut bad, if index == 5 { 4 } else { 6 }, 0);
        assert!(Request::from_bytes(&bad).is_none());
    }
}

#[test]
fn object_results_preserve_pending_retirement_instead_of_claiming_deletion() {
    for reply in results() {
        assert_eq!(Reply::from_bytes(&reply.to_bytes()), Some(reply));
    }
    let cases = [
        (
            kernel_api::abi::memory::Opcode::Flat(kernel_api::abi::memory::flat::Opcode::Delete),
            Value::FlatLifecycle {
                flat: slot(0).flat(),
                version: FlatVersion::INITIAL,
                phase: FlatPhase::Closing,
            },
        ),
        (
            kernel_api::abi::memory::Opcode::Nest(kernel_api::abi::memory::nest::Opcode::Delete),
            Value::NestLifecycle {
                nest: nest_id(),
                version: NestVersion::INITIAL,
                phase: NestPhase::Closing,
            },
        ),
        (
            kernel_api::abi::memory::Opcode::Nest(kernel_api::abi::memory::nest::Opcode::Release),
            Value::SpanLifecycle {
                range: PendId::mint(13),
                version: SpanVersion::INITIAL,
                phase: SpanPhase::Closing,
            },
        ),
    ];
    for (opcode, value) in cases {
        let reply = Reply::new(id(), opcode, Outcome::Pending(value)).unwrap();
        assert!(Reply::new(id(), opcode, Outcome::Committed(value)).is_none());
        assert!(
            Reply::new(
                id(),
                opcode,
                Outcome::Failed {
                    failure: Failure::Hardware,
                    retained: None
                }
            )
            .is_none()
        );
        let mut bad = reply.to_bytes();
        set_word(&mut bad, 4, 2);
        assert!(Reply::from_bytes(&bad).is_none());
    }
    let created = Value::CreatedFlat {
        token: PendId::mint(1),
        flat: slot(0).flat(),
        version: FlatVersion::INITIAL,
    };
    assert!(
        Reply::new(
            id(),
            kernel_api::abi::memory::Opcode::Flat(kernel_api::abi::memory::flat::Opcode::Create),
            Outcome::Pending(created)
        )
        .is_none()
    );
    assert!(
        Reply::new(
            id(),
            kernel_api::abi::memory::Opcode::Nest(kernel_api::abi::memory::nest::Opcode::Create),
            Outcome::Committed(created)
        )
        .is_none()
    );
    assert!(
        Reply::new(
            id(),
            kernel_api::abi::memory::Opcode::Flat(kernel_api::abi::memory::flat::Opcode::Create),
            Outcome::Failed {
                failure: Failure::OutOfMemory,
                retained: Some(created)
            }
        )
        .is_none()
    );
    let invalid = Value::CreatedFlat {
        token: PendId::NONE,
        flat: slot(0).flat(),
        version: FlatVersion::INITIAL,
    };
    assert!(
        Reply::new(
            id(),
            kernel_api::abi::memory::Opcode::Flat(kernel_api::abi::memory::flat::Opcode::Create),
            Outcome::Committed(invalid)
        )
        .is_none()
    );
}

#[test]
fn span_result_carries_the_new_grant_and_both_structural_versions() {
    let result = results()[2];
    let mut expected = [0; Reply::WIDTH];
    for (index, word) in [
        14,
        13,
        19,
        u64::MAX,
        2,
        0,
        7,
        5,
        17,
        1 << 63,
        1 << 63,
        0x8000,
    ]
    .into_iter()
    .enumerate()
    {
        set_word(&mut expected, index, word);
    }
    assert_eq!(result.to_bytes(), expected);
    assert_eq!(Reply::from_bytes(&expected), Some(result));
    for (word, value) in [
        (7, 0),
        (8, 0),
        (9, 0),
        (10, 0),
        (11, 1),
        (11, (usize::MAX & !4095) as u64),
        (12, 1),
    ] {
        let mut bad = expected;
        set_word(&mut bad, word, value);
        assert!(Reply::from_bytes(&bad).is_none());
    }
}

#[test]
fn structural_versions_are_nonzero_full_width_and_never_wrap() {
    assert!(FlatVersion::new(0).is_none());
    assert!(NestVersion::new(0).is_none());
    assert!(SpanVersion::new(0).is_none());
    assert_eq!(FlatVersion::INITIAL.get(), 1);
    assert_eq!(NestVersion::INITIAL.get(), 1);
    assert_eq!(SpanVersion::INITIAL.get(), 1);
    for value in [1, 1 << 63, u64::MAX - 1, u64::MAX] {
        assert_eq!(
            FlatVersion::new(value)
                .unwrap()
                .next()
                .map(FlatVersion::get),
            value.checked_add(1)
        );
        assert_eq!(
            NestVersion::new(value)
                .unwrap()
                .next()
                .map(NestVersion::get),
            value.checked_add(1)
        );
        assert_eq!(
            SpanVersion::new(value)
                .unwrap()
                .next()
                .map(SpanVersion::get),
            value.checked_add(1)
        );
    }
}

#[test]
fn release_range_encodes_identity_expected_revision_and_page_window() {
    let target = kernel_api::model::nest::RangeSlice::new(
        PendId::mint(7),
        kernel_api::address::PageOffset::new(1),
        kernel_api::address::PageCount::new(2).unwrap(),
    )
    .unwrap();
    let request = Request::new(
        Some(id()),
        Action::ReleaseRange {
            target,
            expected: kernel_api::model::nest::RangeVersion::new(3).unwrap(),
        },
    )
    .unwrap();
    let bytes = request.to_bytes();
    for (index, word) in [14, 13, 27, u64::MAX, 7, 1, 2, 3].into_iter().enumerate() {
        assert_eq!(
            u64::from_le_bytes(bytes[index * 8..(index + 1) * 8].try_into().unwrap()),
            word
        );
    }
    assert_eq!(Request::from_bytes(&bytes), Some(request));
    assert_eq!(
        request.action().opcode(),
        kernel_api::abi::memory::Opcode::Nest(kernel_api::abi::memory::nest::Opcode::ReleaseRange)
    );

    let value = Value::SpanLifecycle {
        range: PendId::mint(7),
        version: SpanVersion::new(4).unwrap(),
        phase: SpanPhase::Open,
    };
    let reply = Reply::new(
        id(),
        kernel_api::abi::memory::Opcode::Nest(kernel_api::abi::memory::nest::Opcode::ReleaseRange),
        Outcome::Committed(value),
    )
    .unwrap();
    assert!(reply.matches(request));
}

#[test]
fn release_range_rejects_empty_or_overflowing_windows_and_non_open_replies() {
    let action = |offset, count| {
        let target = kernel_api::model::nest::RangeSlice::new(
            PendId::mint(7),
            kernel_api::address::PageOffset::new(offset),
            kernel_api::address::PageCount::new(count)?,
        )?;
        Some(Action::ReleaseRange {
            target,
            expected: kernel_api::model::nest::RangeVersion::new(3)?,
        })
    };
    assert!(action(0, 0).is_none());
    assert!(action(usize::MAX, 2).is_none());

    let request = Request::new(Some(id()), action(1, 2).unwrap()).unwrap();
    let value = |phase| Value::SpanLifecycle {
        range: PendId::mint(7),
        version: SpanVersion::new(4).unwrap(),
        phase,
    };
    assert!(
        Reply::new(
            id(),
            kernel_api::abi::memory::Opcode::Nest(
                kernel_api::abi::memory::nest::Opcode::ReleaseRange
            ),
            Outcome::Committed(value(SpanPhase::Open))
        )
        .unwrap()
        .matches(request)
    );
    assert!(
        Reply::new(
            id(),
            kernel_api::abi::memory::Opcode::Nest(
                kernel_api::abi::memory::nest::Opcode::ReleaseRange
            ),
            Outcome::Committed(value(SpanPhase::Closed))
        )
        .is_none()
    );
}
