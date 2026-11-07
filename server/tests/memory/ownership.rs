use crate::cell::Owner;
use kernel_api::model::memory::CellOwner;
use std::collections::HashSet;
use std::sync::Arc;

use super::cell::{Cell, CellError};
use super::support::Pool;

#[test]
fn retained_cell_returns_capacity_only_once_after_last_holder() {
    let pool = Pool::new(0);
    let cell = Arc::new(Cell::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
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
    let first = Cell::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap();
    let address = first.physical_address();
    let id = first.id();
    // SAFETY: this test holds the sole allocation, no other access exists.
    unsafe { first.as_mut_ptr().write(0x7F) };
    let moved = first;
    assert_eq!(moved.id(), id);
    drop(moved);
    let second = Cell::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap();
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
        Cell::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap_err(),
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
                    .map(|_| {
                        Cell::from_internal(Owner::Kernel, pool.page().unwrap())
                            .unwrap()
                            .id()
                    })
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

#[test]
fn owner_survives_holders_and_hardware_retirement_until_physical_reuse() {
    use kernel_api::model::identity::TeamId;
    let pool = Pool::new(0);
    let owner = CellOwner::Team(TeamId::new(17));
    let cell = Arc::new(
        Cell::from_internal(Owner::team(TeamId::new(17)).unwrap(), pool.page().unwrap()).unwrap(),
    );
    let identity = cell.id();
    let mut device = cell.prepare_external(false).unwrap();
    device.publish();
    let holder = cell.clone();
    drop(cell);
    assert_eq!(holder.owner(), owner);
    assert_eq!(holder.id(), identity);
    drop(holder);
    assert_eq!(device.cell().owner(), owner);
    assert_eq!(pool.gives(), 0);
    unsafe { device.retire().settle() };
    assert_eq!(pool.gives(), 1);
    let next =
        Cell::from_internal(Owner::team(TeamId::new(18)).unwrap(), pool.page().unwrap()).unwrap();
    assert_ne!(next.id(), identity);
    assert_eq!(next.owner(), CellOwner::Team(TeamId::new(18)));
}

#[test]
fn invalid_owner_does_not_publish_or_leak_the_allocation() {
    use kernel_api::model::identity::TeamId;
    let pool = Pool::new(0);
    assert_eq!(
        Cell::from_internal(Owner::team(TeamId::new(0)).unwrap(), pool.page().unwrap())
            .unwrap_err(),
        CellError::InvalidOwner
    );
    assert_eq!(pool.gives(), 1);
}

#[test]
fn closing_owner_adopts_external_cell_without_reclaiming_it() {
    use kernel_api::model::identity::TeamId;
    let pool = Pool::new(0);
    let owner = Owner::team(TeamId::new(21)).unwrap();
    let cell = Arc::new(Cell::from_internal(owner.clone(), pool.page().unwrap()).unwrap());
    let identity = cell.id();
    let mut device = cell.prepare_external(true).unwrap();
    device.publish();
    owner.close();
    assert_eq!(cell.owner(), CellOwner::Kernel);
    assert_eq!(cell.id(), identity);
    assert_eq!(cell.exposure(), super::cell::Exposure::External);
    drop(cell);
    assert_eq!(pool.gives(), 0);
    unsafe { device.retire().settle() };
    assert_eq!(pool.gives(), 1);
}
