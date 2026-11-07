use super::cell::Cell;
use super::error::FlatError;
use super::index::PageIndex;
use super::slot::{self, Slot};
use super::support::Pool;

#[test]
fn sparse_admission_preserves_identity_and_retained_cells_outlive_slots() {
    let a = Pool::new(0);
    let b = Pool::new(0);
    let first = Cell::from_internal(a.page().unwrap()).unwrap();
    let second = Cell::from_internal(b.page().unwrap()).unwrap();
    let identities = [first.id(), second.id()];
    let slots = slot::admit(
        4,
        vec![(PageIndex::ZERO, first), (PageIndex::new(3), second)],
    )
    .unwrap_or_else(|_| panic!("valid sparse import"));
    let retained: Vec<_> = slots
        .iter()
        .map(|(_, slot)| match slot {
            Slot::Resident(cell) => cell.clone(),
            _ => panic!("an admitted Cell is resident"),
        })
        .collect();
    assert_eq!(
        retained.iter().map(|cell| cell.id()).collect::<Vec<_>>(),
        identities
    );
    drop(slots);
    assert_eq!((a.gives(), b.gives()), (0, 0));
    drop(retained);
    assert_eq!((a.gives(), b.gives()), (1, 1));
}

#[test]
fn invalid_indices_reject_the_entire_import_and_return_all_cells() {
    for indices in [[0, 0], [1, 0], [0, 2]] {
        let pools = [Pool::new(0), Pool::new(0)];
        let cells = indices
            .into_iter()
            .zip(pools)
            .map(|(index, pool)| {
                (
                    PageIndex::new(index),
                    Cell::from_internal(pool.page().unwrap()).unwrap(),
                )
            })
            .collect();
        assert!(matches!(slot::admit(2, cells), Err(FlatError::OutOfRange)));
        assert_eq!((pools[0].gives(), pools[1].gives()), (1, 1));
    }
}
