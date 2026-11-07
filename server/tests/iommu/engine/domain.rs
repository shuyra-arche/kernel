use super::{engine, registers::Device, start};
use crate::iommu::riscv::EngineError;
use crate::iommu::riscv::domain::{Domain, DomainPhase};
use crate::iommu::riscv::registers::*;
use crate::{cell::Cell, support::Pool};

fn domain() -> (Domain<Device>, Device, Vec<&'static Pool>) {
    let (mut engine, device, queues) = engine();
    start(&mut engine);
    let tables = core::array::from_fn::<_, 4, _>(|_| Pool::new(0));
    let payload = Pool::new(0);
    let cells = tables.map(|p| Cell::from_internal(p.page().unwrap()).unwrap());
    let cell = Cell::from_internal(payload.page().unwrap()).unwrap();
    // Emulated register effects, no actual devices, exclusive test-owned Cells.
    let domain = unsafe { Domain::new(engine, cells, cell, 8, 0x400000, true) }.unwrap();
    (
        domain,
        device,
        queues.into_iter().chain(tables).chain([payload]).collect(),
    )
}

fn active(domain: &mut Domain<Device>, device: &Device) {
    domain.start().unwrap();
    assert_eq!(device.read64(DDTP), 0);
    device.complete(true, true);
    assert_eq!(domain.step(), Ok(DomainPhase::Installing));
    assert!(matches!(domain.step().unwrap(), DomainPhase::Starting(_)));
    device.complete(true, true);
    assert_eq!(domain.step(), Ok(DomainPhase::Active));
}

fn closed(domain: &mut Domain<Device>, device: &Device) {
    domain.revoke().unwrap();
    assert_eq!(domain.close(), Err(EngineError::State));
    device.complete(true, true);
    assert_eq!(domain.step(), Ok(DomainPhase::Blocked));
    domain.close().unwrap();
    assert!(matches!(domain.step().unwrap(), DomainPhase::Draining(_)));
    device.complete(true, true);
    assert_eq!(domain.step(), Ok(DomainPhase::Stopping));
    domain.step().unwrap();
    assert_eq!(domain.step(), Ok(DomainPhase::Closed));
}

#[test]
fn translation_completion_cannot_discharge_platform_responsibility() {
    let (mut domain, device, pools) = domain();
    active(&mut domain, &device);
    assert!(pools.iter().all(|p| p.gives() == 0));
    closed(&mut domain, &device);
    assert!(pools[..3].iter().all(|p| p.gives() == 1));
    assert!(pools[3..].iter().all(|p| p.gives() == 0));
    drop(domain);
    // Even dropping a Closed domain is not a platform quiescence proof.
    assert!(
        pools[3..]
            .iter()
            .all(|p| p.gives() == 0 && p.page().is_err())
    );

    let (mut domain, device, pools) = self::domain();
    active(&mut domain, &device);
    closed(&mut domain, &device);
    // No DMA exists in this fixture. Both command batches were completed.
    unsafe { domain.settle() }.unwrap();
    assert!(pools.iter().all(|p| p.gives() == 1));
}

#[test]
fn domain_failure_keeps_both_payload_and_hardware_tables() {
    let (mut domain, device, pools) = domain();
    active(&mut domain, &device);
    domain.revoke().unwrap();
    device.complete(true, true);
    device.set32(CQCSR, ENABLE | ON | (1 << 9));
    assert_eq!(domain.step(), Err(EngineError::Hardware));
    assert_eq!(domain.phase(), DomainPhase::Failed);
    assert_eq!(domain.close(), Err(EngineError::State));
    drop(domain);
    assert!(pools.iter().all(|p| p.gives() == 0 && p.page().is_err()));
}

#[test]
fn invalid_geometry_never_publishes_directory_or_retains_payload() {
    for (id, iova) in [(64, 0x400000), (8, 1), (8, 1usize << 38)] {
        let (mut engine, device, _) = engine();
        start(&mut engine);
        let pools = core::array::from_fn::<_, 5, _>(|_| Pool::new(0));
        let [a, b, c, d, p] = pools.map(|p| Cell::from_internal(p.page().unwrap()).unwrap());
        assert!(matches!(
            unsafe { Domain::new(engine, [a, b, c, d], p, id, iova, true) },
            Err(EngineError::Address)
        ));
        assert_eq!(device.read64(DDTP), 0);
        assert!(pools.iter().all(|p| p.gives() == 1));
    }
}

#[test]
fn fault_queue_wrap_copies_evidence_and_overflow_never_means_completion() {
    let (mut engine, device, pools) = engine();
    start(&mut engine);
    let base = (device.read64(FQB) >> 10) << 12;
    for n in 0..260 {
        let head = n & 127;
        let words = [((8u64) << 40) | (3 << 34) | 15, 0, 0x400000, 0];
        unsafe {
            for (i, word) in words.iter().enumerate() {
                ((base as *mut u64).add(head * 4 + i)).write_volatile(word.to_le());
            }
        }
        device.set32(FQT, ((head + 1) & 127) as u32);
        let fault = engine.fault().unwrap().unwrap();
        assert_eq!(
            (fault.device, fault.cause, fault.transaction, fault.address),
            (8, 15, 3, 0x400000)
        );
        assert!(engine.fault().unwrap().is_none());
    }
    device.set32(FQCSR, ENABLE | ON | (1 << 9));
    assert_eq!(engine.fault(), Err(EngineError::Hardware));
    drop(engine);
    assert!(pools.iter().all(|p| p.gives() == 0));
}
