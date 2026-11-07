use super::hardware::{Call, Scripted, Trace};
use super::iommu::retirement::{Owner, Registry};
use super::iommu::{BatchId, Error, HardwareError};
use super::{cell::Cell, support::Pool};
use crate::cell::Owner as CellOwner;
use std::sync::{Arc, Mutex};

fn latest(trace: &Arc<Mutex<Trace>>) -> BatchId {
    trace
        .lock()
        .unwrap()
        .calls
        .iter()
        .rev()
        .find_map(|call| {
            if let Call::Fence(_, batch) = call {
                Some(*batch)
            } else {
                None
            }
        })
        .unwrap()
}

#[test]
fn owner_exit_needs_no_record_transfer_and_failed_domain_does_not_block_another() {
    let mut registry = Registry::new();
    let owner = Owner::new().unwrap();
    let pools = [Pool::new(0), Pool::new(0)];
    let mut traces = Vec::new();
    let mut ids = Vec::new();
    for pool in pools {
        let (hardware, trace) = Scripted::new();
        let domain = registry
            .attach(&owner, hardware)
            .map_err(|(e, _)| e)
            .unwrap();
        let cell = Arc::new(Cell::from_internal(CellOwner::Kernel, pool.page().unwrap()).unwrap());
        registry
            .with_space(&owner, domain, |space| {
                let binding = unsafe { space.register(&cell, 0x400000, true) }?;
                space.publish(binding)
            })
            .unwrap();
        drop(cell);
        ids.push(domain);
        traces.push(trace);
    }
    assert_eq!(registry.step(0).visited, 0);
    owner.close();
    assert_eq!(
        registry.with_space(&owner, ids[0], |_| Ok(())),
        Err(Error::Closed)
    );
    std::thread::spawn(move || drop(owner)).join().unwrap();
    assert_eq!(registry.pending(), 2);
    assert!(traces.iter().all(|t| t.lock().unwrap().drops == 0));
    // A one-entry budget visits both domains over successive ticks.
    assert_eq!(registry.step(1).visited, 1);
    assert_eq!(registry.step(1).visited, 1);
    let old = latest(&traces[0]);
    let second = latest(&traces[1]);
    traces[0]
        .lock()
        .unwrap()
        .completion
        .push_back(Err(HardwareError::Timeout));
    traces[1]
        .lock()
        .unwrap()
        .completion
        .push_back(Ok(Some(second)));
    assert_eq!(registry.step(1).failed, 1);
    assert_eq!(
        registry.error(ids[0]),
        Ok(Some(Error::Hardware(HardwareError::Timeout)))
    );
    assert_eq!(registry.step(1).reclaimed, 1);
    assert_eq!((pools[0].gives(), pools[1].gives()), (0, 1));
    assert_eq!(registry.pending(), 1);
    registry.step(1); // retry allocates a fresh revocation batch
    let fresh = latest(&traces[0]);
    assert_ne!(old, fresh);
    traces[0]
        .lock()
        .unwrap()
        .completion
        .push_back(Ok(Some(old)));
    assert_eq!(registry.step(1).failed, 1);
    assert_eq!(registry.error(ids[0]), Ok(Some(Error::WrongBatch)));
    assert_eq!(pools[0].gives(), 0);
    traces[0]
        .lock()
        .unwrap()
        .completion
        .push_back(Ok(Some(fresh)));
    assert_eq!(registry.step(4).reclaimed, 1);
    assert_eq!(registry.pending(), 0);
    assert!(pools.iter().all(|p| p.gives() == 1));
    assert!(traces.iter().all(|t| t.lock().unwrap().drops == 1));
}

#[test]
fn owner_and_domain_identity_cannot_be_reused_as_authority() {
    let mut registry = Registry::new();
    let owner = Owner::new().unwrap();
    let other = Owner::new().unwrap();
    let (hardware, trace) = Scripted::new();
    let id = registry
        .attach(&owner, hardware)
        .map_err(|(e, _)| e)
        .unwrap();
    assert_eq!(
        registry.with_space(&other, id, |_| Ok(())),
        Err(Error::UnknownDomain)
    );
    let pool = Pool::new(0);
    let cell = Arc::new(Cell::from_internal(CellOwner::Kernel, pool.page().unwrap()).unwrap());
    registry
        .with_space(&owner, id, |space| unsafe {
            space.register(&cell, 0x400000, false)
        })
        .unwrap();
    drop(cell);
    owner.close();
    registry.step(1);
    assert_eq!(registry.pending(), 0);
    assert_eq!(pool.gives(), 1);
    assert!(trace.lock().unwrap().calls.is_empty()); // unpublished cancellation
    let (hardware, _) = Scripted::new();
    let (_, hardware) = registry
        .attach(&owner, hardware)
        .err()
        .expect("closed owner");
    let replacement = registry
        .attach(&other, hardware)
        .map_err(|(e, _)| e)
        .unwrap();
    assert_ne!(replacement, id);
    assert_eq!(
        registry.with_space(&other, id, |_| Ok(())),
        Err(Error::UnknownDomain)
    );
    drop(other);
    assert_eq!(registry.step(1).reclaimed, 1);
}

#[test]
fn publication_admitted_before_concurrent_exit_remains_in_kernel_custody() {
    let mut registry = Registry::new();
    let owner = Owner::new().unwrap();
    let (hardware, trace) = Scripted::new();
    let id = registry
        .attach(&owner, hardware)
        .map_err(|(e, _)| e)
        .unwrap();
    let pool = Pool::new(0);
    let cell = Arc::new(Cell::from_internal(CellOwner::Kernel, pool.page().unwrap()).unwrap());
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            barrier.wait(); // operation has passed the registry admission check
            owner.close();
            barrier.wait();
        });
        registry
            .with_space(&owner, id, |space| {
                barrier.wait();
                barrier.wait(); // exit precedes the actual hardware publication
                let binding = unsafe { space.register(&cell, 0x400000, true) }?;
                space.publish(binding)
            })
            .unwrap();
    });
    drop(cell);
    assert_eq!(pool.gives(), 0);
    assert_eq!(
        registry.with_space(&owner, id, |_| Ok(())),
        Err(Error::Closed)
    );
    registry.step(1);
    let batch = latest(&trace);
    trace.lock().unwrap().completion.push_back(Ok(Some(batch)));
    assert_eq!(registry.step(1).reclaimed, 1);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn explicit_close_settles_only_its_receipt_without_requiring_owner_exit() {
    let mut registry = Registry::new();
    let owner = Owner::new().unwrap();
    let (hardware, trace) = Scripted::new();
    let id = registry
        .attach(&owner, hardware)
        .map_err(|(e, _)| e)
        .unwrap();
    let receipt = registry.completion(id).unwrap();
    let (other_hardware, _) = Scripted::new();
    let other = registry
        .attach(&owner, other_hardware)
        .map_err(|(e, _)| e)
        .unwrap();
    let other_receipt = registry.completion(other).unwrap();
    let pool = Pool::new(0);
    let cell = Arc::new(Cell::from_internal(CellOwner::Kernel, pool.page().unwrap()).unwrap());
    registry
        .with_space(&owner, id, |space| {
            let binding = unsafe { space.register(&cell, 0x400000, true) }?;
            space.publish(binding)?;
            space.close()
        })
        .unwrap();
    drop(cell);
    registry.step(4);
    assert!(!receipt.settled());
    assert_eq!(pool.gives(), 0);
    let batch = latest(&trace);
    trace.lock().unwrap().completion.push_back(Ok(Some(batch)));
    assert_eq!(registry.step(4).reclaimed, 1);
    assert!(receipt.settled());
    assert!(!other_receipt.settled());
    assert!(registry.completion(id).is_none());
    assert_eq!(pool.gives(), 1);
    assert!(!owner.closed());
    owner.close();
    registry.step(4);
    assert!(other_receipt.settled());
}
