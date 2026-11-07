use crate::cell::Owner as CellOwner;
use std::sync::{Arc, Barrier};

use super::cell::{Cell, Exposure, RetainError};
use super::hardware::{Call, Scripted};
use super::iommu::{Error, HardwareError, IoSpace, Life, Phase};
use super::support::Pool;

#[test]
fn preparation_is_external_before_publication_and_cancellation_has_no_effects() {
    let pool = Pool::new(0);
    let cell = Arc::new(Cell::from_internal(CellOwner::Kernel, pool.page().unwrap()).unwrap());
    let (hardware, trace) = Scripted::new();
    let mut space = IoSpace::new(hardware);
    assert_eq!(cell.exposure(), Exposure::Internal);
    // SAFETY: no hardware, concurrent payload accesses or carrier migration exist.
    let id = unsafe { space.register(&cell, 0x1000, true) }.unwrap();
    assert_eq!(cell.exposure(), Exposure::External);
    assert!(!cell.allows_return());
    assert!(trace.lock().unwrap().calls.is_empty());
    assert_eq!(space.revoke(id), Ok(None));
    assert!(trace.lock().unwrap().calls.is_empty());
    assert_eq!(cell.exposure(), Exposure::Internal);
    assert!(cell.allows_return());
    drop(cell);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn closing_retains_external_memory_until_its_batch_actually_completes() {
    let pool = Pool::new(0);
    let cell = Arc::new(Cell::from_internal(CellOwner::Kernel, pool.page().unwrap()).unwrap());
    let (hardware, trace) = Scripted::new();
    let mut space = IoSpace::new(hardware);
    // SAFETY: no actual hardware or other payload access.
    let id = unsafe { space.register(&cell, 0x4000, true) }.unwrap();
    space.publish(id).unwrap();
    space.close().unwrap();
    assert_eq!(space.life(), Life::Closing);
    assert_eq!(space.publish(id), Err(Error::Closed));
    // SAFETY: only attempts admission, with no actual hardware.
    assert_eq!(
        unsafe { space.register(&cell, 0x8000, false) },
        Err(Error::Closed)
    );
    let Phase::Waiting(batch) = space.phase(id).unwrap() else {
        panic!("waiting")
    };
    assert_eq!(space.revoke(id), Ok(Some(batch)));
    assert_eq!(space.poll(id), Ok(false));
    let weak = Arc::downgrade(&cell);
    drop(cell); // Requesting Unit's last ordinary hold has ended.
    assert_eq!(weak.upgrade().unwrap().exposure(), Exposure::External);
    assert_eq!(pool.gives(), 0);
    assert!(pool.page().is_err());
    trace.lock().unwrap().completion.push_back(Ok(Some(batch)));
    assert_eq!(space.poll(id), Ok(true));
    assert!(weak.upgrade().is_none());
    assert_eq!(pool.gives(), 1);
    assert_eq!(space.life(), Life::Closed);
    assert_eq!(space.pending(), 0);
    assert_eq!(space.poll(id), Err(Error::UnknownBinding));
    space.close().unwrap();
    drop(space);
    let trace = trace.lock().unwrap();
    assert_eq!(trace.drops, 1);
    assert!(
        matches!(trace.calls.as_slice(), [Call::Publish(p), Call::Revoke(r), Call::Fence(f, b), Call::Observe(wait), Call::Observe(done)]
        if p.id == id && p.cell == r.cell && p.physical == f.physical && p.iova == 0x4000
        && p.modifiable && p.size == 4096 && *b == batch && *wait == batch && *done == batch)
    );
}

#[test]
fn external_settlement_neither_discharges_another_device_nor_cpu_access() {
    let pool = Pool::new(0);
    let cell = Arc::new(Cell::from_internal(CellOwner::Kernel, pool.page().unwrap()).unwrap());
    let mut cpu = cell.prepare_hardware(false).unwrap();
    cpu.publish();
    let (hardware, trace) = Scripted::new();
    let mut space = IoSpace::new(hardware);
    // SAFETY: no actual hardware or payload access; read-only aliases are modeled.
    let a = unsafe { space.register(&cell, 0x1000, false) }.unwrap();
    let b = unsafe { space.register(&cell, 0x2000, false) }.unwrap();
    space.publish(a).unwrap();
    space.publish(b).unwrap();
    let first = space.revoke(a).unwrap().unwrap();
    let second = space.revoke(b).unwrap().unwrap();
    trace.lock().unwrap().completion.push_back(Ok(Some(first)));
    space.poll(a).unwrap();
    assert_eq!(cell.exposure(), Exposure::External);
    trace.lock().unwrap().completion.push_back(Ok(Some(second)));
    space.poll(b).unwrap();
    assert_eq!(cell.exposure(), Exposure::Internal);
    assert!(!cell.allows_return());
    drop(cell);
    assert_eq!(pool.gives(), 0);
    // SAFETY: no actual CPU mapping exists in this host scenario.
    unsafe { cpu.retire().settle() };
    assert_eq!(pool.gives(), 1);
}

#[test]
fn partial_publication_failure_requires_revocation_and_cannot_be_cancelled() {
    let pool = Pool::new(0);
    let cell = Arc::new(Cell::from_internal(CellOwner::Kernel, pool.page().unwrap()).unwrap());
    let (hardware, trace) = Scripted::new();
    let mut space = IoSpace::new(hardware);
    // SAFETY: no actual hardware or payload access.
    let id = unsafe { space.register(&cell, 0, true) }.unwrap();
    trace.lock().unwrap().fail_publish = Some(HardwareError::Fault);
    assert_eq!(
        space.publish(id),
        Err(Error::Hardware(HardwareError::Fault))
    );
    assert_eq!(space.phase(id), Ok(Phase::Failed));
    assert_eq!(cell.exposure(), Exposure::External);
    assert_eq!(space.publish(id), Err(Error::InvalidState));
    let batch = space.revoke(id).unwrap().unwrap();
    drop(cell);
    assert_eq!(pool.gives(), 0);
    trace.lock().unwrap().completion.push_back(Ok(Some(batch)));
    assert_eq!(space.poll(id), Ok(true));
    assert_eq!(pool.gives(), 1);
}

#[test]
fn close_rejects_new_work_but_advances_other_bindings_when_one_revoke_fails() {
    let pools = [Pool::new(0), Pool::new(0)];
    let cells: Vec<_> = pools
        .iter()
        .map(|pool| Arc::new(Cell::from_internal(CellOwner::Kernel, pool.page().unwrap()).unwrap()))
        .collect();
    let (hardware, trace) = Scripted::new();
    let mut space = IoSpace::new(hardware);
    let ids: Vec<_> = cells
        .iter()
        .enumerate()
        .map(|(i, cell)| {
            // SAFETY: separate cells with no actual hardware or payload access.
            let id = unsafe { space.register(cell, i * 4096, true) }.unwrap();
            space.publish(id).unwrap();
            id
        })
        .collect();
    trace.lock().unwrap().fail_revoke = Some(HardwareError::Unavailable);
    assert_eq!(
        space.close(),
        Err(Error::Hardware(HardwareError::Unavailable))
    );
    assert_eq!(space.life(), Life::Closing);
    assert_eq!(space.phase(ids[0]), Ok(Phase::Failed));
    assert!(matches!(space.phase(ids[1]), Ok(Phase::Waiting(_))));
    assert!(
        cells
            .iter()
            .all(|cell| cell.exposure() == Exposure::External)
    );
    space.close().unwrap();
    drop(cells);
    for id in ids {
        let Phase::Waiting(batch) = space.phase(id).unwrap() else {
            panic!("waiting")
        };
        trace.lock().unwrap().completion.push_back(Ok(Some(batch)));
        space.poll(id).unwrap();
    }
    assert_eq!(space.life(), Life::Closed);
    assert!(pools.iter().all(|pool| pool.gives() == 1));
}

#[test]
fn abandoned_space_retains_hardware_and_cell_but_unpublished_space_can_cancel() {
    for published in [false, true] {
        let pool = Pool::new(0);
        let cell = Arc::new(Cell::from_internal(CellOwner::Kernel, pool.page().unwrap()).unwrap());
        let (hardware, trace) = Scripted::new();
        let mut space = IoSpace::new(hardware);
        // SAFETY: no actual hardware or payload access.
        let id = unsafe { space.register(&cell, 0, false) }.unwrap();
        if published {
            space.publish(id).unwrap();
        }
        drop(space);
        if published {
            assert_eq!(cell.exposure(), Exposure::External);
            assert!(matches!(
                cell.prepare_external(false),
                Err(RetainError::Quarantined)
            ));
            assert_eq!(trace.lock().unwrap().drops, 0);
        } else {
            assert_eq!(cell.exposure(), Exposure::Internal);
            assert!(cell.allows_return());
            assert_eq!(trace.lock().unwrap().drops, 1);
        }
        drop(cell);
        assert_eq!(pool.gives(), usize::from(!published));
    }
}

#[test]
fn mixed_cpu_device_concurrency_never_clears_the_remaining_external_use() {
    let pool = Pool::new(0);
    let cell = Arc::new(Cell::from_internal(CellOwner::Kernel, pool.page().unwrap()).unwrap());
    let mut remaining = cell.prepare_external(false).unwrap();
    remaining.publish();
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let cell = cell.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..64 {
                    let mut retained = if i % 2 == 0 {
                        cell.prepare_external(false)
                    } else {
                        cell.prepare_hardware(false)
                    }
                    .unwrap();
                    retained.publish();
                    // SAFETY: no actual hardware or payload access in this host test.
                    unsafe { retained.retire().settle() };
                    assert_eq!(cell.exposure(), Exposure::External);
                    assert!(!cell.allows_return());
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    // SAFETY: no actual hardware or payload access.
    unsafe { remaining.retire().settle() };
    assert_eq!(cell.exposure(), Exposure::Internal);
    assert!(cell.allows_return());
    drop(cell);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn writable_dma_rejects_cpu_aliases_until_the_exact_retirement_completes() {
    let pool = Pool::new(0);
    let cell = Arc::new(Cell::from_internal(CellOwner::Kernel, pool.page().unwrap()).unwrap());
    let cpu = cell.prepare_hardware(false).unwrap();
    let (hardware, trace) = Scripted::new();
    let mut space = IoSpace::new(hardware);
    // No hardware accesses exist; exercise admission using the real registry.
    assert_eq!(
        unsafe { space.register(&cell, 0x1000, true) },
        Err(Error::Retain(RetainError::Borrowed))
    );
    assert_eq!(space.pending(), 0);
    assert!(trace.lock().unwrap().calls.is_empty());
    drop(cpu);
    let id = unsafe { space.register(&cell, 0x1000, true) }.unwrap();
    space.publish(id).unwrap();
    let batch = space.revoke(id).unwrap().unwrap();
    assert!(!space.poll(id).unwrap());
    assert!(matches!(
        cell.prepare_hardware(false),
        Err(RetainError::Borrowed)
    ));
    assert!(matches!(
        cell.prepare_external(false),
        Err(RetainError::Borrowed)
    ));
    trace.lock().unwrap().completion.push_back(Ok(Some(batch)));
    assert!(space.poll(id).unwrap());
    assert_eq!(cell.exposure(), Exposure::Internal);
    let cpu = cell.prepare_hardware(true).unwrap();
    drop(cpu);
    assert!(cell.allows_return());
}
