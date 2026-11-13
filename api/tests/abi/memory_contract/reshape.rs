use super::request::{id, maintenance, set_word};
use super::slot;
use kernel_api::{
    abi::memory::{Action, Failure, Maintenance, Opcode, Outcome, Reply, Request, Value},
    address::BASE_PAGE_SIZE,
    model::{
        cell::CellLevel,
        flat::{BlockRef, FlatId, FlatVersion, Reshape, SlotVersion},
        identity::PendId,
    },
};

pub(super) fn mutations() -> Vec<Action> {
    let start = slot(512);
    let expected = FlatVersion::new((1 << 63) - 1).unwrap();
    let parent = CellLevel::new(1).unwrap();
    let maintenance = maintenance();
    vec![
        Action::Split {
            start,
            expected,
            parent,
            maintenance,
        },
        Action::Merge {
            start,
            expected,
            parent,
            maintenance,
        },
    ]
}

fn geometry() -> Value {
    Value::Geometry {
        flat: slot(0).flat(),
        version: FlatVersion::new(1 << 63).unwrap(),
        slots: SlotVersion::new(1 << 63).unwrap(),
        range: Reshape::new(512, CellLevel::new(1).unwrap()).unwrap(),
    }
}

pub(super) fn results() -> Vec<Reply> {
    [Opcode::Split, Opcode::Merge]
        .into_iter()
        .map(|opcode| Reply::new(id(), opcode, Outcome::Committed(geometry())).unwrap())
        .collect()
}

#[test]
fn reshape_vectors_cover_exact_parent_range_and_aggregate_version() {
    for (action, operation) in mutations().into_iter().zip([25, 26]) {
        let request = Request::new(Some(id()), action).unwrap();
        let mut expected = [0; Request::WIDTH];
        for (index, word) in [
            11,
            12,
            operation,
            u64::MAX,
            7,
            11,
            512,
            (1 << 63) - 1,
            1,
            23,
            11,
            29,
            31,
        ]
        .into_iter()
        .enumerate()
        {
            set_word(&mut expected, index, word);
        }
        assert_eq!(request.to_bytes(), expected);
        assert_eq!(
            Request::from_call(action.opcode().call_number(), &expected),
            Some(request)
        );
        assert!(Request::from_call(Opcode::Protect.call_number(), &expected).is_none());
        assert!(Request::new(None, action).is_none());
        let mut result_bytes = [0; Reply::WIDTH];
        for (index, word) in [
            11,
            12,
            operation,
            u64::MAX,
            2,
            0,
            11,
            11,
            1 << 63,
            1 << 63,
            512,
            1,
        ]
        .into_iter()
        .enumerate()
        {
            set_word(&mut result_bytes, index, word);
        }
        let reply = Reply::new(id(), action.opcode(), Outcome::Committed(geometry())).unwrap();
        assert_eq!(reply.to_bytes(), result_bytes);
        assert_eq!(Reply::from_bytes(&result_bytes), Some(reply));
        assert!(reply.matches(request));
    }
}

#[test]
fn reshape_never_accepts_a_partial_parent_or_a_foreign_maintenance_record() {
    for action in mutations() {
        let bytes = Request::new(Some(id()), action).unwrap().to_bytes();
        for (index, value) in [
            (4, 0),
            (5, 0),
            (6, 513),
            (6, u64::MAX),
            (7, 0),
            (8, 0),
            (8, 7),
            (9, 0),
            (10, 12),
            (11, 0),
            (12, 0),
            (13, 1),
        ] {
            let mut bad = bytes;
            set_word(&mut bad, index, value);
            assert!(
                Request::from_bytes(&bad).is_none(),
                "word {index}, value {value}"
            );
        }
    }
    for parent in [CellLevel::BASE, CellLevel::new(1).unwrap()] {
        let invalid = Action::Split {
            start: slot(513),
            expected: FlatVersion::INITIAL,
            parent,
            maintenance: maintenance(),
        };
        assert!(Request::new(Some(id()), invalid).is_none());
    }
    let mut foreign = maintenance();
    foreign.block =
        BlockRef::new(FlatId::new(37).unwrap(), 29, foreign.block.generation()).unwrap();
    for m in [
        foreign,
        Maintenance {
            token: PendId::NONE,
            ..maintenance()
        },
    ] {
        assert!(
            Request::new(
                Some(id()),
                Action::Merge {
                    start: slot(512),
                    expected: FlatVersion::INITIAL,
                    parent: CellLevel::new(1).unwrap(),
                    maintenance: m,
                }
            )
            .is_none()
        );
    }
}

#[test]
fn immediate_children_partition_all_representable_levels_without_rebasing() {
    let mut parent = CellLevel::new(1).unwrap();
    loop {
        let count = parent.pages().unwrap();
        let range = Reshape::new(count, parent).unwrap();
        let child_count = range.child().pages().unwrap();
        assert_eq!(range.count(), count);
        assert_eq!(range.offset(), count);
        assert_eq!(range.parent(), parent);
        assert_eq!(child_count * Reshape::CHILDREN, count);
        for child in 0..Reshape::CHILDREN {
            let head = range.child_offset(child).unwrap();
            assert_eq!(head, count + child * child_count);
            assert!(slot(head).fits(count, count, range.child()));
        }
        assert_eq!(range.child_offset(511).unwrap() + child_count, count * 2);
        assert!(range.child_offset(512).is_none());
        assert!(range.child_offset(usize::MAX).is_none());
        assert!(range.fits(count, count));
        assert!(range.fits(count - 1, count + 1));
        assert!(!range.fits(count + 1, count));
        assert!(!range.fits(count, count - 1));
        assert!(!range.fits(0, count));
        assert!(Reshape::new(count + 1, parent).is_none());
        let top = (usize::MAX / BASE_PAGE_SIZE) & !(count - 1);
        assert!(Reshape::new(top, parent).is_none());
        let Some(next) = parent.parent() else { break };
        parent = next;
    }
    assert!(Reshape::new(0, CellLevel::BASE).is_none());
}

#[test]
fn flat_high_water_marks_do_not_reissue_retired_child_versions() {
    // Numeric contract only: this does not simulate a Server allocation change.
    // A parent's local successor (3) could collide with a former child's stamp.
    let old_parent = SlotVersion::new(2).unwrap();
    let retired_child = SlotVersion::new(3).unwrap();
    assert_eq!(old_parent.next(), Some(retired_child));
    let high_water = FlatVersion::new(900).unwrap();
    let (merged, parent_stamp) = high_water.next_slots().unwrap();
    let (split, child_stamp) = merged.next_slots().unwrap();
    assert_eq!((merged.get(), parent_stamp.get()), (901, 901));
    assert_eq!((split.get(), child_stamp.get()), (902, 902));
    assert!(child_stamp > parent_stamp && parent_stamp > retired_child);
    assert_eq!(FlatVersion::INITIAL.next_slots().unwrap().1.get(), 2);
    let (last, slots) = FlatVersion::new(u64::MAX - 1)
        .unwrap()
        .next_slots()
        .unwrap();
    assert_eq!(last.get(), u64::MAX);
    assert_eq!(slots.get(), u64::MAX);
    assert!(last.next_slots().is_none());
}

#[test]
fn reshape_results_cannot_claim_partial_geometry_or_pending_allocation_changes() {
    for opcode in [Opcode::Split, Opcode::Merge] {
        assert!(Reply::new(id(), opcode, Outcome::Pending(geometry())).is_none());
        assert!(
            Reply::new(
                id(),
                opcode,
                Outcome::Failed {
                    failure: Failure::OutOfMemory,
                    retained: Some(geometry()),
                }
            )
            .is_none()
        );
        assert!(
            Reply::new(
                id(),
                opcode,
                Outcome::Failed {
                    failure: Failure::OutOfMemory,
                    retained: None,
                }
            )
            .is_some()
        );
        assert!(
            Reply::new(
                id(),
                opcode,
                Outcome::Committed(Value::Slot {
                    version: SlotVersion::new(2).unwrap(),
                    page: 53,
                })
            )
            .is_none()
        );
        let bytes = Reply::new(id(), opcode, Outcome::Committed(geometry()))
            .unwrap()
            .to_bytes();
        for (index, value) in [
            (4, 1),
            (7, 0),
            (8, 0),
            (8, 3),
            (9, 0),
            (9, 3),
            (10, 513),
            (11, 0),
            (11, 7),
            (12, 1),
        ] {
            let mut bad = bytes;
            set_word(&mut bad, index, value);
            assert!(
                Reply::from_bytes(&bad).is_none(),
                "word {index}, value {value}"
            );
        }
        for stamp in [1, 2, u64::MAX] {
            let mut bytes = bytes;
            set_word(&mut bytes, 8, stamp);
            set_word(&mut bytes, 9, stamp);
            assert_eq!(Reply::from_bytes(&bytes).is_some(), stamp > 1);
        }
    }
}

#[test]
fn reshape_receipts_correlate_the_range_and_new_epoch_to_the_original_request() {
    for (action, reply) in mutations().into_iter().zip(results()) {
        let request = Request::new(Some(id()), action).unwrap();
        for (index, value) in [(7, 37), (10, 1024)] {
            let mut bytes = reply.to_bytes();
            set_word(&mut bytes, index, value);
            let other = Reply::from_bytes(&bytes).unwrap();
            assert!(!other.matches(request));
        }
        let mut bytes = reply.to_bytes();
        set_word(&mut bytes, 8, 2);
        set_word(&mut bytes, 9, 2);
        assert!(!Reply::from_bytes(&bytes).unwrap().matches(request));
        let mut bytes = request.to_bytes();
        set_word(&mut bytes, 7, u64::MAX);
        assert!(!reply.matches(Request::from_bytes(&bytes).unwrap()));
    }
}
