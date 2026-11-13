use super::fetch_words;
use kernel_api::{
    abi::{
        call::{
            EnvCall,
            cell::{
                CellCall,
                operations::{Create, Delete},
            },
        },
        operation::{KernelCall, RegisterCall},
    },
    address::VirtAddr,
    model::{cell::*, identity::PendId},
};

fn slot(id: u64) -> SlotId {
    SlotId::from_bytes(&id.to_le_bytes()).unwrap()
}

#[test]
fn objects_and_positions_have_compact_checked_layouts() {
    assert_eq!(CellObject::WIDTH, 16);
    assert_eq!(CellPosition::WIDTH, 32);
    for (object, tag, id) in [
        (CellObject::Slot(slot(1)), 0, 1),
        (CellObject::Pend(PendId::mint(7)), 1, 7),
    ] {
        let position = CellPosition {
            object,
            page: 3,
            generation: if matches!(object, CellObject::Slot(_)) {
                0
            } else {
                u64::MAX
            },
        };
        assert_eq!(fetch_words::<2>(&object.to_bytes()).unwrap(), [tag, id]);
        assert_eq!(CellObject::from_bytes(&object.to_bytes()), Some(object));
        assert_eq!(
            fetch_words::<4>(&position.to_bytes()).unwrap(),
            [tag, id, 3, position.generation as usize]
        );
        assert_eq!(
            CellPosition::from_bytes(&position.to_bytes()),
            Some(position)
        );
        for (word, invalid) in [(0, 2usize), (1, 0), (1, usize::MAX), (2, usize::MAX)] {
            let mut bad = position.to_bytes();
            bad[word * 8..(word + 1) * 8].copy_from_slice(&invalid.to_le_bytes());
            assert_eq!(CellPosition::from_bytes(&bad), None);
        }
        assert_eq!(CellObject::from_bytes(&[0; 15]), None);
        assert_eq!(CellObject::from_bytes(&[0; 17]), None);
        assert_eq!(CellPosition::from_bytes(&position.to_bytes()[..31]), None);
        assert_eq!(CellPosition::from_bytes(&[0; 33]), None);
    }
    let position = CellPosition {
        object: CellObject::Slot(slot(1)),
        page: 0,
        generation: 1,
    };
    assert_eq!(CellPosition::from_bytes(&position.to_bytes()), None);
    for words in [
        [0usize, 0],
        [1, 0],
        [2, 1],
        [0, usize::MAX],
        [1, usize::MAX],
    ] {
        let bytes: Vec<_> = words.into_iter().flat_map(usize::to_le_bytes).collect();
        assert_eq!(CellObject::from_bytes(&bytes), None);
    }
    assert_eq!(CellObject::from_bytes(&[0; 32]), None);
    assert_eq!(CellPosition::from_bytes(&[0; 48]), None);
}

#[test]
fn handoffs_cannot_turn_a_live_pend_into_an_owning_slot() {
    assert_eq!(CellHandoff::WIDTH, 64);
    assert_eq!(CellAssociation::WIDTH, 64);
    let local = CellPosition {
        object: CellObject::Slot(slot(1)),
        page: 2,
        generation: 0,
    };
    let remote = CellPosition {
        object: CellObject::Slot(slot(3)),
        page: 4,
        generation: 0,
    };
    let handoff = CellHandoff { local, remote };
    assert_eq!(
        fetch_words::<8>(&handoff.to_bytes()).unwrap(),
        [0, 1, 2, 0, 0, 3, 4, 0]
    );
    assert_eq!(CellHandoff::from_bytes(&handoff.to_bytes()), Some(handoff));
    for generation in [0, 1, u64::MAX] {
        let pend = CellPosition {
            object: CellObject::Pend(PendId::mint(7)),
            generation,
            ..remote
        };
        for handoff in [
            CellHandoff {
                local,
                remote: pend,
            },
            CellHandoff {
                local: pend,
                remote,
            },
            CellHandoff {
                local: pend,
                remote: pend,
            },
        ] {
            assert_eq!(CellHandoff::from_bytes(&handoff.to_bytes()), None);
        }
        for source in [local, pend] {
            let association = CellAssociation {
                source,
                target: pend,
            };
            assert_eq!(
                CellAssociation::from_bytes(&association.to_bytes()),
                Some(association)
            );
        }
    }
    for len in 0..CellHandoff::WIDTH {
        assert_eq!(CellHandoff::from_bytes(&handoff.to_bytes()[..len]), None);
        assert_eq!(
            CellAssociation::from_bytes(&handoff.to_bytes()[..len]),
            None
        );
    }
    assert_eq!(CellHandoff::from_bytes(&[0; 65]), None);
    assert_eq!(CellAssociation::from_bytes(&[0; 65]), None);
    assert_eq!(CellHandoff::from_bytes(&[0; 96]), None);
    assert_eq!(CellAssociation::from_bytes(&[0; 96]), None);
}

#[test]
fn create_allocates_only_a_slot_and_delete_takes_that_handle_directly() {
    let allocation = Allocation::at_level(CellLevel::new(1).unwrap(), 2).unwrap();
    let create = Create {
        license: PendId::mint(17),
        allocation,
    };
    assert_eq!(Create::SLOT, kernel_api::abi::PREFIX | (2usize << 32));
    assert_eq!(
        create.pack(),
        [17, 4 * 1024 * 1024, 2 * 1024 * 1024, 1, 0, 0]
    );
    assert_eq!(Create::from_registers(&create.pack()), Ok(create));
    assert_eq!(
        EnvCall::from_wire(Create::SLOT, &create.pack()),
        Ok(EnvCall::Cell(CellCall::Create {
            license: create.license,
            allocation
        }))
    );
    let made: SlotId = Create::decode_return((23, 99, 0x6000)).unwrap();
    assert_eq!(made, slot(23));
    let delete = Delete { slot: made };
    assert_eq!(delete.pack(), [23, 0, 0, 0, 0, 0]);
    assert_eq!(Delete::from_registers(&delete.pack()), Ok(delete));
    assert_eq!(Delete::decode_return((0, 0, 0)).unwrap(), ());
    for words in [
        [17, 0x4000, 0, 0, 0, 0],
        [17, 0, 4096, 0, 0, 0],
        [17, 4096, 3, 0, 0, 0],
        [17, 4096, 4096, usize::MAX, 0, 0],
        [17, 4096, 4096, 1, 0, 0],
    ] {
        assert!(Create::from_registers(&words).is_err());
    }
}

#[test]
fn observation_preserves_generation_and_full_width_faults() {
    let info = CellInfo {
        level: CellLevel::BASE,
        generation: u64::MAX,
        state: CellState::Retracting,
        page: 7,
        frozen: false,
        external: true,
        address: VirtAddr::new(0x8000),
        fault_cause: u64::MAX,
        fault_address: u64::MAX,
    };
    let bytes = info.to_bytes();
    assert_eq!(CellInfo::WIDTH, 64);
    assert_eq!(CellInfo::from_bytes(&bytes), Some(info));
    for (offset, value) in [(8, 9), (24, 4), (56, 255)] {
        let mut bad = bytes;
        bad[offset] = value;
        assert!(CellInfo::from_bytes(&bad).is_none());
    }
}
