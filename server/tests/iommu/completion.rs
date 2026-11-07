use crate::cell::Owner as CellOwner;
use std::sync::Arc;

use super::cell::{Cell, Exposure};
use super::hardware::Scripted;
use super::iommu::{Error, HardwareError, IoSpace, Phase, riscv};
use super::support::Pool;

#[test]
fn old_retry_completion_and_cross_domain_identity_cannot_release_memory() {
    let pool = Pool::new(0);
    let cell = Arc::new(Cell::from_internal(CellOwner::Kernel, pool.page().unwrap()).unwrap());
    let (hardware, trace) = Scripted::new();
    let (other, other_trace) = Scripted::new();
    let mut space = IoSpace::new(hardware);
    let mut other_space = IoSpace::new(other);
    // SAFETY: no hardware or payload access; both are read-only projections.
    let id = unsafe { space.register(&cell, 0, false) }.unwrap();
    let other_id = unsafe { other_space.register(&cell, 0, false) }.unwrap();
    space.publish(id).unwrap();
    other_space.publish(other_id).unwrap();
    assert_ne!(id, other_id);
    assert_eq!(space.revoke(other_id), Err(Error::UnknownBinding));
    let old = space.revoke(id).unwrap().unwrap();
    let other_batch = other_space.revoke(other_id).unwrap().unwrap();
    trace
        .lock()
        .unwrap()
        .completion
        .push_back(Err(HardwareError::Timeout));
    assert_eq!(space.poll(id), Err(Error::Hardware(HardwareError::Timeout)));
    assert_eq!(space.phase(id), Ok(Phase::Failed));
    let new = space.revoke(id).unwrap().unwrap();
    assert_ne!(old, new);
    assert_ne!(new, other_batch);
    for stale in [old, other_batch] {
        trace.lock().unwrap().completion.push_back(Ok(Some(stale)));
        assert_eq!(space.poll(id), Err(Error::WrongBatch));
        assert_eq!(space.phase(id), Ok(Phase::Waiting(new)));
        assert_eq!(cell.exposure(), Exposure::External);
        assert_eq!(pool.gives(), 0);
    }
    trace.lock().unwrap().completion.push_back(Ok(Some(new)));
    space.poll(id).unwrap();
    assert_eq!(cell.exposure(), Exposure::External);
    other_trace
        .lock()
        .unwrap()
        .completion
        .push_back(Ok(Some(other_batch)));
    other_space.poll(other_id).unwrap();
    assert_eq!(cell.exposure(), Exposure::Internal);
    drop(cell);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn fence_submission_failure_keeps_iova_reserved_until_retry_settlement() {
    let pool = Pool::new(0);
    let cell = Arc::new(Cell::from_internal(CellOwner::Kernel, pool.page().unwrap()).unwrap());
    let (hardware, trace) = Scripted::new();
    let mut space = IoSpace::new(hardware);
    // SAFETY: no hardware or payload access.
    let id = unsafe { space.register(&cell, 4096, true) }.unwrap();
    space.publish(id).unwrap();
    trace.lock().unwrap().fail_fence = Some(HardwareError::Unavailable);
    assert_eq!(
        space.revoke(id),
        Err(Error::Hardware(HardwareError::Unavailable))
    );
    assert_eq!(space.phase(id), Ok(Phase::Failed));
    // SAFETY: rejected before producing any hardware effect.
    assert_eq!(
        unsafe { space.register(&cell, 4096, false) },
        Err(Error::Overlap)
    );
    let batch = space.revoke(id).unwrap().unwrap();
    trace.lock().unwrap().completion.push_back(Ok(Some(batch)));
    space.poll(id).unwrap();
    // SAFETY: no actual hardware; the earlier relationship is fully settled.
    let reused = unsafe { space.register(&cell, 4096, false) }.unwrap();
    assert_ne!(id, reused);
    assert_eq!(space.poll(id), Err(Error::UnknownBinding));
    assert_eq!(cell.exposure(), Exposure::External);
    space.revoke(reused).unwrap();
    drop(cell);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn rejected_geometry_does_not_register_external_risk() {
    let pool = Pool::new(0);
    let cell = Arc::new(Cell::from_internal(CellOwner::Kernel, pool.page().unwrap()).unwrap());
    let (hardware, trace) = Scripted::new();
    let mut space = IoSpace::new(hardware);
    for iova in [1, usize::MAX - 4095] {
        // SAFETY: rejected preparation with no hardware or payload accesses.
        assert_eq!(
            unsafe { space.register(&cell, iova, false) },
            Err(Error::InvalidRange)
        );
        assert_eq!(cell.exposure(), Exposure::Internal);
        assert!(cell.allows_return());
    }
    assert_eq!(space.pending(), 0);
    assert!(trace.lock().unwrap().calls.is_empty());
    drop(cell);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn riscv_fence_encoding_and_error_status_do_not_confuse_notification_with_success() {
    let pool = Pool::new(0);
    let cell = Arc::new(Cell::from_internal(CellOwner::Kernel, pool.page().unwrap()).unwrap());
    let (hardware, trace) = Scripted::new();
    let mut space = IoSpace::new(hardware);
    // SAFETY: no hardware or payload accesses; obtains a real production batch.
    let id = unsafe { space.register(&cell, 0, true) }.unwrap();
    space.publish(id).unwrap();
    let batch = space.revoke(id).unwrap().unwrap();
    assert_eq!(riscv::Command::invalidate_host().words(), [1, 0]);
    assert_eq!(
        riscv::Command::fence(batch, 0x8000_1000, false)
            .unwrap()
            .words(),
        [(u64::from(batch.value()) << 32) | 0x1402, 0x2000_0400]
    );
    assert_eq!(
        riscv::Command::fence(batch, 0x8000_1000, true)
            .unwrap()
            .words(),
        [(u64::from(batch.value()) << 32) | 0x3402, 0x2000_0400]
    );
    assert_eq!(
        riscv::Command::fence(batch, 3, true),
        Err(Error::InvalidRange)
    );
    let on = 1 << 16;
    assert_eq!(riscv::fence_complete(batch, on, batch.value()), Ok(true));
    assert_eq!(riscv::fence_complete(batch, on | (1 << 11), 0), Ok(false));
    assert_eq!(riscv::fence_complete(batch, 0, batch.value()), Ok(false));
    assert_eq!(
        riscv::fence_complete(batch, on | (1 << 17), batch.value()),
        Ok(false)
    );
    for bit in [8, 10] {
        assert_eq!(
            riscv::fence_complete(batch, on | (1 << bit), batch.value()),
            Err(HardwareError::Fault)
        );
    }
    assert_eq!(
        riscv::fence_complete(batch, on | (1 << 9), batch.value()),
        Err(HardwareError::Timeout)
    );
    // CQ samples alone never settle the production space.
    assert_eq!(cell.exposure(), Exposure::External);
    trace.lock().unwrap().completion.push_back(Ok(Some(batch)));
    space.poll(id).unwrap();
    drop(cell);
    assert_eq!(pool.gives(), 1);
}
