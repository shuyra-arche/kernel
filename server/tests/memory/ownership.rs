use std::collections::HashSet;
use std::sync::Arc;

use super::cell::{Cell, CellError};
use super::support::Pool;

#[test]
fn retained_cell_returns_capacity_only_once_after_last_holder() {
    let pool = Pool::new(0);
    let cell = Arc::new(Cell::from_internal(pool.page().unwrap()).unwrap());
    let id = cell.id();
    let retained = cell.clone();
    drop(cell);
    assert_eq!(pool.gives(), 0);
    assert!(pool.page().is_err());
    assert_eq!(retained.id(), id);
    std::thread::spawn(move || drop(retained)).join().unwrap();
    assert_eq!(pool.gives(), 1);
}

#[test]
fn reused_physical_page_gets_new_identity_and_zeroed_contents() {
    let pool = Pool::new(0);
    let first = Cell::from_internal(pool.page().unwrap()).unwrap();
    let address = first.physical_address();
    let id = first.id();
    // SAFETY: this test holds the sole allocation, no other access exists.
    unsafe { first.as_mut_ptr().write(0x7F) };
    let moved = first;
    assert_eq!(moved.id(), id);
    drop(moved);
    let second = Cell::from_internal(pool.page().unwrap()).unwrap();
    assert_eq!(second.physical_address(), address);
    assert_ne!(second.id(), id);
    // SAFETY: the allocation is live and held exclusively by this test.
    assert_eq!(unsafe { second.as_mut_ptr().read() }, 0);
    drop(second);
    assert_eq!(pool.gives(), 2);
}

#[test]
fn rejected_physical_range_returns_its_allocation() {
    // A valid byte-aligned Rust allocation need not be a valid physical page.
    let pool = Pool::new(1);
    assert_eq!(
        Cell::from_internal(pool.page().unwrap()).unwrap_err(),
        CellError::InvalidRange
    );
    assert_eq!(pool.gives(), 1);
    assert!(pool.page().is_ok());
    assert_eq!(pool.gives(), 2);
}

#[test]
fn concurrent_instances_never_share_an_identity() {
    let threads: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                let pool = Pool::new(0);
                (0..32)
                    .map(|_| Cell::from_internal(pool.page().unwrap()).unwrap().id())
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let ids: Vec<_> = threads
        .into_iter()
        .flat_map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(ids.len(), 256);
    assert_eq!(ids.iter().copied().collect::<HashSet<_>>().len(), ids.len());
}
