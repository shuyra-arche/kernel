//! Exercise bounded production progress independently from a Task's lifetime.
use crate::cell::{Exposure, Owner, Page, RetainError};
use crate::hardware::{Call, Scripted};
use crate::iommu::{Error, HardwareError};
use crate::publication::Phase;
use crate::support::Pool;
use crate::target::Target;
use std::sync::Arc;

#[test]
fn one_failed_target_does_not_discharge_or_block_another() {
    let pools = [Pool::new(0), Pool::new(0)];
    let mut targets = Vec::new();
    let mut traces = Vec::new();
    for pool in pools {
        let (hardware, trace) = Scripted::new();
        let mut target = Target::new(hardware);
        let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
        target.prepare(&page, 4096, true, None).unwrap();
        target.publish().unwrap();
        drop(page); // no requester-owned Page reference remains
        targets.push(target);
        traces.push(trace);
    }
    assert!(crate::cell::statistics::snapshot().external >= 8192);
    traces[0].lock().unwrap().fail_revoke = Some(HardwareError::Timeout);
    assert_eq!(
        targets[0].advance_close(),
        Err(Error::Hardware(HardwareError::Timeout))
    );
    assert!(!targets[1].advance_close().unwrap());
    let Some(Phase::Waiting(second)) = targets[1].phase() else {
        panic!("batch")
    };
    traces[1]
        .lock()
        .unwrap()
        .completion
        .push_back(Ok(Some(second)));
    assert!(targets[1].advance_close().unwrap());
    assert!(targets[1].advance_close().unwrap());
    assert_eq!((pools[0].gives(), pools[1].gives()), (0, 1));
    assert!(!targets[0].advance_close().unwrap());
    let Some(Phase::Waiting(first)) = targets[0].phase() else {
        panic!("batch")
    };
    traces[0]
        .lock()
        .unwrap()
        .completion
        .push_back(Ok(Some(second)));
    assert_eq!(targets[0].advance_close(), Err(Error::WrongBatch));
    assert_eq!(pools[0].gives(), 0);
    traces[0]
        .lock()
        .unwrap()
        .completion
        .push_back(Ok(Some(first)));
    assert!(targets[0].advance_close().unwrap());
    drop(targets);
    assert!(pools.iter().all(|p| p.gives() == 1));
    assert!(traces.iter().all(|t| t.lock().unwrap().drops == 1));
}

#[test]
fn close_cancels_preparation_and_rejects_new_publication() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let (hardware, trace) = Scripted::new();
    let mut target = Target::new(hardware);
    target.prepare(&page, 4096, true, None).unwrap();
    assert!(target.advance_close().unwrap());
    assert_eq!(target.publish(), Err(Error::Closed));
    assert_eq!(target.prepare(&page, 8192, true, None), Err(Error::Closed));
    assert!(target.advance_close().unwrap());
    assert!(trace.lock().unwrap().calls.is_empty());
    assert!(page.allows_return());
    drop((target, page));
    assert_eq!(pool.gives(), 1);
    assert_eq!(trace.lock().unwrap().drops, 1);
}

#[test]
fn lost_target_quarantines_published_payload_and_retains_backend_storage() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let (hardware, trace) = Scripted::new();
    let mut target = Target::new(hardware);
    target.prepare(&page, 4096, false, None).unwrap();
    target.publish().unwrap();
    drop(target);
    assert_eq!(page.exposure(), Exposure::External);
    assert!(matches!(
        page.prepare_external(false),
        Err(RetainError::Quarantined)
    ));
    assert_eq!(trace.lock().unwrap().drops, 0);
    drop(page);
    assert_eq!(pool.gives(), 0);
}

#[test]
fn shutdown_failure_retains_backend_after_payload_is_already_safe() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let (hardware, trace) = Scripted::new();
    let mut target = Target::new(hardware);
    target.prepare(&page, 4096, true, None).unwrap();
    target.publish().unwrap();
    let batch = target.revoke().unwrap().unwrap();
    trace
        .lock()
        .unwrap()
        .shutdown
        .push_back(Err(HardwareError::Timeout));
    trace.lock().unwrap().completion.push_back(Ok(Some(batch)));
    assert!(target.poll().unwrap());
    // The payload settles independently of the context and queue shutdown.
    assert_eq!(
        target.advance_close(),
        Err(Error::Hardware(HardwareError::Timeout))
    );
    assert_eq!(target.phase(), None);
    assert!(!target.settled());
    drop(page);
    assert_eq!(pool.gives(), 1);
    trace.lock().unwrap().shutdown.push_back(Ok(false));
    assert!(!target.advance_close().unwrap());
    assert_eq!(target.phase(), None);
    assert!(target.advance_close().unwrap());
    assert_eq!(target.phase(), None);
    drop(target);
    assert_eq!(trace.lock().unwrap().drops, 1);
    // All publication/withdrawal calls named exactly the same Page and address.
    let trace = trace.lock().unwrap();
    assert!(
        matches!(trace.calls.as_slice(), [Call::Publish(p), Call::Revoke(r), Call::Fence(f, b), Call::Observe(seen)]
        if p == r && r == f && p.iova.get() == 4096 && *b == batch && *seen == batch)
    );
}
