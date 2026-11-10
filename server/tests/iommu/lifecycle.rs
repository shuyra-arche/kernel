use crate::cell::{Exposure, Owner as PageOwner, Page, RetainError};
use crate::hardware::Scripted;
use crate::iommu::{Error, HardwareError};
use crate::publication::{Phase, Publication};
use crate::support::Pool;
use std::sync::{Arc, Barrier};

#[test]
fn prepared_cancellation_never_publishes_hardware() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(PageOwner::Kernel, pool.page().unwrap()).unwrap());
    let (mut hardware, trace) = Scripted::new();
    let mut publication =
        Publication::prepare_external(&hardware, &page, 4096, true, None).unwrap();
    assert_eq!(page.exposure(), Exposure::External);
    assert!(!page.allows_return());
    assert_eq!(publication.revoke(&mut hardware), Ok(None));
    assert!(trace.lock().unwrap().calls.is_empty());
    assert_eq!(page.exposure(), Exposure::Internal);
    drop(page);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn partial_publish_requires_withdrawal_even_after_the_requester_drops_its_page() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(PageOwner::Kernel, pool.page().unwrap()).unwrap());
    let (mut hardware, trace) = Scripted::new();
    let mut publication =
        Publication::prepare_external(&hardware, &page, 4096, true, None).unwrap();
    trace.lock().unwrap().fail_publish = Some(HardwareError::Fault);
    assert_eq!(
        publication.submit(&mut hardware),
        Err(Error::Hardware(HardwareError::Fault))
    );
    assert_eq!(publication.phase(), Phase::Failed);
    assert_eq!(publication.submit(&mut hardware), Err(Error::InvalidState));
    let batch = publication.revoke(&mut hardware).unwrap().unwrap();
    drop(page);
    assert!(!publication.poll(&mut hardware).unwrap());
    assert_eq!(pool.gives(), 0);
    assert!(pool.page().is_err());
    trace.lock().unwrap().completion.push_back(Ok(Some(batch)));
    assert!(publication.poll(&mut hardware).unwrap());
    assert_eq!(pool.gives(), 1);
}

#[test]
fn external_settlement_cannot_discharge_a_cpu_publication() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(PageOwner::Kernel, pool.page().unwrap()).unwrap());
    let mut cpu = Publication::prepare(&page, false, None).unwrap();
    cpu.publish();
    cpu.complete();
    let (mut hardware, trace) = Scripted::new();
    let mut external = Publication::prepare_external(&hardware, &page, 4096, false, None).unwrap();
    external.submit(&mut hardware).unwrap();
    let batch = external.revoke(&mut hardware).unwrap().unwrap();
    trace.lock().unwrap().completion.push_back(Ok(Some(batch)));
    assert!(external.poll(&mut hardware).unwrap());
    assert_eq!(page.exposure(), Exposure::Internal);
    assert!(!page.allows_return());
    drop(page);
    assert_eq!(pool.gives(), 0);
    // No real CPU translation exists in this host fixture.
    unsafe { cpu.retire().settle() };
    assert_eq!(pool.gives(), 1);
}

#[test]
fn writable_external_access_excludes_cpu_aliases_until_exact_completion() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(PageOwner::Kernel, pool.page().unwrap()).unwrap());
    let cpu = Publication::prepare(&page, false, None).unwrap();
    let (mut hardware, trace) = Scripted::new();
    assert!(matches!(
        Publication::prepare_external(&hardware, &page, 4096, true, None),
        Err(Error::Retain(RetainError::Borrowed))
    ));
    drop(cpu);
    let mut external = Publication::prepare_external(&hardware, &page, 4096, true, None).unwrap();
    external.submit(&mut hardware).unwrap();
    let batch = external.revoke(&mut hardware).unwrap().unwrap();
    assert!(!external.poll(&mut hardware).unwrap());
    assert!(matches!(
        Publication::prepare(&page, false, None),
        Err(RetainError::Borrowed)
    ));
    assert!(matches!(
        page.prepare_external(false),
        Err(RetainError::Borrowed)
    ));
    trace.lock().unwrap().completion.push_back(Ok(Some(batch)));
    assert!(external.poll(&mut hardware).unwrap());
    drop(Publication::prepare(&page, true, None).unwrap());
    assert!(page.allows_return());
}

#[test]
fn a_change_authorizes_only_its_exact_page_and_does_not_end_external_access() {
    let pool = Pool::new(0);
    let other_pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(PageOwner::Kernel, pool.page().unwrap()).unwrap());
    let other =
        Arc::new(Page::from_internal(PageOwner::Kernel, other_pool.page().unwrap()).unwrap());
    let change = page.begin().unwrap();
    let freeze = crate::cell::Freeze::new(change.clone(), None).unwrap();
    let (mut hardware, trace) = Scripted::new();
    assert!(matches!(
        Publication::prepare_external(&hardware, &page, 4096, true, None),
        Err(Error::Retain(RetainError::Busy))
    ));
    assert!(matches!(
        Publication::prepare_external(&hardware, &other, 4096, true, Some((&change, false))),
        Err(Error::InvalidRange)
    ));
    assert!(trace.lock().unwrap().calls.is_empty());
    let mut external = Publication::prepare_external(
        &hardware,
        &page,
        4096,
        true,
        Some((&change, freeze.belongs_to(&change))),
    )
    .unwrap();
    external.submit(&mut hardware).unwrap();
    drop((freeze, change));
    assert_eq!(page.exposure(), Exposure::External);
    assert_eq!(
        page.prepare_hardware(false).unwrap_err(),
        RetainError::Borrowed
    );
    let batch = external.revoke(&mut hardware).unwrap().unwrap();
    drop(page);
    assert_eq!(pool.gives(), 0);
    trace.lock().unwrap().completion.push_back(Ok(Some(batch)));
    assert!(external.poll(&mut hardware).unwrap());
    assert_eq!(pool.gives(), 1);
}

#[test]
fn mixed_cpu_device_concurrency_never_clears_the_remaining_external_use() {
    let pool = Pool::new(0);
    let cell = Arc::new(Page::from_internal(PageOwner::Kernel, pool.page().unwrap()).unwrap());
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
