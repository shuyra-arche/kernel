//! Next-memory-ABI value contracts. These do not exercise server authorization,
//! publication, maintenance or retirement, and do not activate a new call ABI.
mod reply;
mod request;

use kernel_api::{
    abi::{self, call::EnvCall},
    address::BASE_PAGE_SIZE,
    model::{
        cell::{CellLevel, MappingId},
        flat::{BlockGeneration, BlockRef, FlatId, SlotPosition, SlotVersion},
        identity::PendId,
        nest::{MemoGeneration, MemoPosition, NestId},
    },
};

fn slot(page: usize) -> SlotPosition {
    SlotPosition::new(PendId::mint(7), FlatId::new(11).unwrap(), page).unwrap()
}

fn memo(page: usize) -> MemoPosition {
    MemoPosition::new(
        PendId::mint(13),
        NestId::new(17).unwrap(),
        MappingId::from_bytes(&19u64.to_le_bytes()).unwrap(),
        page,
    )
    .unwrap()
}

#[test]
fn positions_encode_identity_separately_from_expected_revision() {
    let slot = slot(512);
    assert_eq!(SlotPosition::WIDTH, 24);
    assert_eq!(
        super::fetch_words::<3>(&slot.to_bytes()),
        Some([7, 11, 512])
    );
    assert_eq!(SlotPosition::from_bytes(&slot.to_bytes()), Some(slot));
    assert_eq!(
        (slot.token().get(), slot.flat().get(), slot.page()),
        (7, 11, 512)
    );
    let memo = memo(512);
    assert_eq!(MemoPosition::WIDTH, 32);
    assert_eq!(
        super::fetch_words::<4>(&memo.to_bytes()),
        Some([13, 17, 19, 512])
    );
    assert_eq!(MemoPosition::from_bytes(&memo.to_bytes()), Some(memo));
    assert_eq!(
        (
            memo.token().get(),
            memo.nest().get(),
            memo.mapping().get(),
            memo.page()
        ),
        (13, 17, 19, 512)
    );
    // A byte order check independent of the crate's encoder/decoder round trip.
    assert_eq!(&slot.to_bytes()[16..24], &[0, 2, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn narrowing_never_rebases_coordinates_and_must_cover_the_whole_page() {
    let large = CellLevel::new(1).unwrap();
    for (offset, count, expected) in [
        (0, 1024, true),
        (512, 512, true),
        (511, 513, true),
        (513, 512, false),
        (0, 512, false),
        (512, 511, false),
        (512, 0, false),
        (usize::MAX, 2, false),
        (0, usize::MAX / BASE_PAGE_SIZE + 1, false),
    ] {
        assert_eq!(slot(512).fits(offset, count, large), expected);
        assert_eq!(memo(512).fits(offset, count, large), expected);
    }
    // Coordinate zero must not become coordinate 512 after narrowing.
    assert!(!slot(0).fits(512, 512, large));
    assert!(!memo(0).fits(512, 512, large));
    // A head inside the window is not sufficient when geometry is indivisible.
    assert!(!slot(513).fits(512, 1024, large));
    assert!(!memo(513).fits(512, 1024, large));
    assert!(slot(513).fits(512, 2, CellLevel::BASE));
    assert!(memo(513).fits(512, 2, CellLevel::BASE));
}

#[test]
fn position_decoders_reject_sentinels_overflow_and_nonexact_lengths() {
    let high = isize::MAX as u64 + 1;
    for (word, value) in [
        (0, 0),
        (0, high),
        (1, 0),
        (1, high),
        (2, u64::MAX),
        (2, usize::MAX as u64 / BASE_PAGE_SIZE as u64),
    ] {
        let mut bytes = slot(0).to_bytes();
        bytes[word * 8..(word + 1) * 8].copy_from_slice(&value.to_le_bytes());
        assert!(
            SlotPosition::from_bytes(&bytes).is_none(),
            "word {word}, {value}"
        );
    }
    for (word, value) in [
        (0, 0),
        (0, high),
        (1, 0),
        (1, high),
        (2, 0),
        (2, high),
        (3, u64::MAX),
        (3, usize::MAX as u64 / BASE_PAGE_SIZE as u64),
    ] {
        let mut bytes = memo(0).to_bytes();
        bytes[word * 8..(word + 1) * 8].copy_from_slice(&value.to_le_bytes());
        assert!(
            MemoPosition::from_bytes(&bytes).is_none(),
            "word {word}, {value}"
        );
    }
    let last = usize::MAX / BASE_PAGE_SIZE - 1;
    assert!(slot(last).fits(last, 1, CellLevel::BASE));
    assert!(memo(last).fits(last, 1, CellLevel::BASE));
    for len in 0..SlotPosition::WIDTH {
        assert!(SlotPosition::from_bytes(&slot(0).to_bytes()[..len]).is_none());
    }
    for len in 0..MemoPosition::WIDTH {
        assert!(MemoPosition::from_bytes(&memo(0).to_bytes()[..len]).is_none());
    }
    assert!(SlotPosition::from_bytes(&[0; SlotPosition::WIDTH + 1]).is_none());
    assert!(MemoPosition::from_bytes(&[0; MemoPosition::WIDTH + 1]).is_none());
    assert!(SlotPosition::new(PendId::NONE, FlatId::new(1).unwrap(), 0).is_none());
    assert!(
        MemoPosition::new(PendId::mint(1), NestId::new(1).unwrap(), MappingId::NONE, 0).is_none()
    );
}

#[test]
fn versions_preserve_full_u64_and_never_wrap() {
    assert!(SlotVersion::new(0).is_none());
    assert!(MemoGeneration::new(0).is_none());
    assert!(BlockGeneration::new(0).is_none());
    assert_eq!(SlotVersion::INITIAL.get(), 1);
    assert_eq!(MemoGeneration::INITIAL.get(), 1);
    assert_eq!(BlockGeneration::INITIAL.get(), 1);
    for value in [1, 2, 1 << 63, u64::MAX - 1, u64::MAX] {
        let slot = SlotVersion::new(value).unwrap();
        let memo = MemoGeneration::new(value).unwrap();
        let block = BlockGeneration::new(value).unwrap();
        assert_eq!(slot.to_bytes(), value.to_le_bytes());
        assert_eq!(memo.to_bytes(), value.to_le_bytes());
        assert_eq!(SlotVersion::from_bytes(&value.to_le_bytes()), Some(slot));
        assert_eq!(MemoGeneration::from_bytes(&value.to_le_bytes()), Some(memo));
        assert_eq!(slot.next().map(SlotVersion::get), value.checked_add(1));
        assert_eq!(memo.next().map(MemoGeneration::get), value.checked_add(1));
        assert_eq!(block.next().map(BlockGeneration::get), value.checked_add(1));
    }
    for bytes in [&[0u8; 7][..], &[0; 8], &[1; 9]] {
        assert!(SlotVersion::from_bytes(bytes).is_none());
        assert!(MemoGeneration::from_bytes(bytes).is_none());
    }
}

#[test]
fn maintenance_reference_preserves_full_width_record_and_generation() {
    let flat = FlatId::new(11).unwrap();
    let generation = BlockGeneration::new(u64::MAX).unwrap();
    let block = BlockRef::new(flat, u64::MAX, generation).unwrap();
    assert_eq!(BlockRef::WIDTH, 24);
    let mut expected = [0; 24];
    expected[..8].copy_from_slice(&11u64.to_le_bytes());
    expected[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
    expected[16..].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(block.to_bytes(), expected);
    assert_eq!(BlockRef::from_bytes(&expected), Some(block));
    assert_eq!(
        (block.flat(), block.record(), block.generation()),
        (flat, u64::MAX, generation)
    );
    assert!(BlockRef::new(flat, 0, generation).is_none());
    for (word, value) in [(0, 0), (0, isize::MAX as u64 + 1), (1, 0), (2, 0)] {
        let mut bad = expected;
        bad[word * 8..(word + 1) * 8].copy_from_slice(&value.to_le_bytes());
        assert!(BlockRef::from_bytes(&bad).is_none());
    }
    for len in 0..BlockRef::WIDTH {
        assert!(BlockRef::from_bytes(&expected[..len]).is_none());
    }
    assert!(BlockRef::from_bytes(&[0; BlockRef::WIDTH + 1]).is_none());
}

#[test]
fn preparing_value_contracts_does_not_enable_next_version_dispatch() {
    assert_eq!(abi::VERSION, 10);
    for domain in [2usize, 12, 13] {
        for operation in 0..=32 {
            assert!(EnvCall::from_wire((11 << 48) | (domain << 32) | operation, &[0; 6]).is_err());
        }
    }
}
