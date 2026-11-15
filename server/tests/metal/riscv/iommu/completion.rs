use crate::cell::{Exposure, Owner, Page};
use crate::hardware::Scripted;
use crate::iommu as riscv;
use crate::iommu::{Error, HardwareError};
use crate::publication::{Phase, Publication};
use crate::support::Pool;
use std::sync::Arc;

#[test]
fn old_retry_and_foreign_domain_batches_cannot_release_the_page() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let (mut a, ta) = Scripted::new();
    let (mut b, tb) = Scripted::new();
    let mut first = Publication::prepare_external(&a, &page, 4096, false, None).unwrap();
    let mut second = Publication::prepare_external(&b, &page, 4096, false, None).unwrap();
    first.submit(&mut a).unwrap();
    second.submit(&mut b).unwrap();
    let old = first.revoke(&mut a).unwrap().unwrap();
    let foreign = second.revoke(&mut b).unwrap().unwrap();
    ta.lock()
        .unwrap()
        .completion
        .push_back(Err(HardwareError::Timeout));
    assert_eq!(
        first.poll(&mut a),
        Err(Error::Hardware(HardwareError::Timeout))
    );
    let fresh = first.revoke(&mut a).unwrap().unwrap();
    assert_ne!(old, fresh);
    assert_ne!(fresh, foreign);
    for stale in [old, foreign] {
        ta.lock().unwrap().completion.push_back(Ok(Some(stale)));
        assert_eq!(first.poll(&mut a), Err(Error::WrongBatch));
        assert_eq!(first.phase(), Phase::Waiting(fresh));
        assert_eq!(page.exposure(), Exposure::External);
        assert_eq!(pool.gives(), 0);
    }
    ta.lock().unwrap().completion.push_back(Ok(Some(fresh)));
    assert!(first.poll(&mut a).unwrap());
    assert_eq!(page.exposure(), Exposure::External);
    tb.lock().unwrap().completion.push_back(Ok(Some(foreign)));
    assert!(second.poll(&mut b).unwrap());
    assert_eq!(page.exposure(), Exposure::Internal);
    assert_eq!(first.poll(&mut a), Err(Error::InvalidState));
    drop(page);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn failed_submission_retains_the_page_and_retry_gets_a_new_batch() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let (mut hardware, trace) = Scripted::new();
    let mut publication =
        Publication::prepare_external(&hardware, &page, 4096, true, None).unwrap();
    publication.submit(&mut hardware).unwrap();
    trace.lock().unwrap().fail_fence = Some(HardwareError::Unavailable);
    assert_eq!(
        publication.revoke(&mut hardware),
        Err(Error::Hardware(HardwareError::Unavailable))
    );
    assert_eq!(publication.phase(), Phase::Failed);
    assert!(!page.allows_return());
    let batch = publication.revoke(&mut hardware).unwrap().unwrap();
    assert_eq!(publication.revoke(&mut hardware), Ok(Some(batch)));
    drop(page);
    assert_eq!(pool.gives(), 0);
    trace.lock().unwrap().completion.push_back(Ok(Some(batch)));
    assert!(publication.poll(&mut hardware).unwrap());
    assert_eq!(pool.gives(), 1);
}

#[test]
fn rejected_geometry_has_no_external_obligation_or_hardware_effect() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let (hardware, trace) = Scripted::new();
    for address in [1, usize::MAX - 4095] {
        assert!(matches!(
            Publication::prepare_external(&hardware, &page, address, false, None),
            Err(Error::InvalidRange)
        ));
        assert_eq!(page.exposure(), Exposure::Internal);
        assert!(page.allows_return());
    }
    assert!(trace.lock().unwrap().calls.is_empty());
}

#[test]
fn command_and_irq_observations_are_not_publication_completion() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let (mut hardware, trace) = Scripted::new();
    let mut publication = Publication::prepare_external(&hardware, &page, 0, true, None).unwrap();
    publication.submit(&mut hardware).unwrap();
    let batch = publication.revoke(&mut hardware).unwrap().unwrap();
    assert_eq!(riscv::Command::invalidate_host().words(), [1, 0]);
    for (interrupt, flags) in [(false, 0x1402), (true, 0x3402)] {
        assert_eq!(
            riscv::Command::fence(batch, 0x8000_1000, interrupt)
                .unwrap()
                .words(),
            [(u64::from(batch.value()) << 32) | flags, 0x2000_0400]
        );
    }
    assert_eq!(
        riscv::Command::fence(batch, 3, true),
        Err(Error::InvalidRange)
    );
    let on = 1 << 16;
    assert_eq!(riscv::fence_complete(batch, on, batch.value()), Ok(true));
    for status in [0, on | (1 << 17)] {
        assert_eq!(
            riscv::fence_complete(batch, status, batch.value()),
            Ok(false)
        );
    }
    assert_eq!(riscv::fence_complete(batch, on | (1 << 11), 0), Ok(false));
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
    assert_eq!(page.exposure(), Exposure::External);
    trace.lock().unwrap().completion.push_back(Ok(Some(batch)));
    assert!(publication.poll(&mut hardware).unwrap());
}
