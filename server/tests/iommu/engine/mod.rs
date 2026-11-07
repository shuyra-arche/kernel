mod domain;
mod registers;

use crate::cell::Owner as PageOwner;
use crate::cell::Page;
use crate::iommu::riscv::Engine;
use crate::iommu::riscv::EngineError;
use crate::iommu::riscv::Phase;
use crate::iommu::riscv::registers::BUSY;
use crate::iommu::riscv::registers::CQCSR;
use crate::iommu::riscv::registers::DDTP;
use crate::iommu::riscv::registers::ENABLE;
use crate::iommu::riscv::registers::FQCSR;
use crate::iommu::riscv::registers::ON;
use crate::iommu::riscv::registers::Registers;
use crate::support::Pool;
use registers::Device;

fn engine() -> (Engine<Device>, Device, [&'static Pool; 3]) {
    let pools = [Pool::new(0), Pool::new(0), Pool::new(0)];
    let cells =
        pools.map(|pool| Page::from_internal(PageOwner::Kernel, pool.page().unwrap()).unwrap());
    let device = Device::new();
    (Engine::new(device.clone(), cells).unwrap(), device, pools)
}

fn start(engine: &mut Engine<Device>) {
    assert_eq!(engine.step(), Ok(Phase::StartingFaults));
    assert_eq!(engine.step(), Ok(Phase::StartingCommands));
    assert_eq!(engine.step(), Ok(Phase::Ready));
}

#[test]
fn incomplete_fence_and_busy_shutdown_cannot_return_hardware_pages() {
    let (mut engine, mut device, pools) = engine();
    start(&mut engine);
    let batch = engine.invalidate().unwrap();
    assert_eq!(engine.stop(), Err(EngineError::State));
    device.complete(false, true);
    assert_eq!(engine.observe(batch), Ok(false));
    device.complete(true, false);
    assert_eq!(engine.observe(batch), Ok(true));
    engine.stop().unwrap();
    device.set32(CQCSR, ON | BUSY);
    assert_eq!(engine.step(), Ok(Phase::StoppingCommands));
    assert!(pools.iter().all(|p| p.gives() == 0));
    device.set32(CQCSR, 0);
    assert_eq!(engine.step(), Ok(Phase::StoppingFaults));
    device.set32(FQCSR, ON | BUSY);
    assert_eq!(engine.step(), Ok(Phase::StoppingFaults));
    assert!(pools.iter().all(|p| p.gives() == 0));
    device.set32(FQCSR, 0);
    device.write64(DDTP, 16);
    assert_eq!(engine.step(), Ok(Phase::StoppingFaults));
    assert!(pools.iter().all(|p| p.gives() == 0));
    device.write64(DDTP, 0);
    assert_eq!(engine.step(), Ok(Phase::Closed));
    assert!(pools.iter().all(|p| p.gives() == 1));
    drop(engine);
    assert!(pools.iter().all(|p| p.gives() == 1));
}

#[test]
fn ring_wrap_and_stale_completion_do_not_complete_the_next_batch() {
    let (mut engine, device, _) = engine();
    start(&mut engine);
    let old = engine.invalidate().unwrap();
    device.complete(true, true);
    assert_eq!(engine.observe(old), Ok(true));
    for _ in 0..300 {
        let batch = engine.invalidate().unwrap();
        assert_ne!(batch, old);
        assert_eq!(engine.observe(old), Err(EngineError::WrongBatch));
        device.complete(true, false);
        assert_eq!(engine.observe(batch), Ok(false));
        device.complete(true, true);
        assert_eq!(engine.observe(batch), Ok(true));
    }
    engine.stop().unwrap();
    engine.step().unwrap();
    assert_eq!(engine.step(), Ok(Phase::Closed));
}

#[test]
fn errors_and_abandoned_engine_keep_allocations_but_preparation_cancels() {
    let (engine, _, pools) = engine();
    drop(engine);
    assert!(pools.iter().all(|p| p.gives() == 1));
    let (mut engine, device, pools) = self::engine();
    start(&mut engine);
    let batch = engine.invalidate().unwrap();
    device.complete(true, true);
    device.set32(CQCSR, ENABLE | ON | (1 << 9));
    assert_eq!(engine.observe(batch), Err(EngineError::Hardware));
    assert_eq!(engine.phase(), Phase::Failed);
    assert_eq!(engine.stop(), Err(EngineError::State));
    drop(engine);
    assert!(pools.iter().all(|p| p.gives() == 0 && p.page().is_err()));
}

#[test]
fn active_hardware_is_rejected_before_register_mutation_or_publication() {
    let (mut engine, mut device, pools) = engine();
    device.write64(DDTP, 1); // Bare is not a safe reset state for this adapter.
    assert_eq!(engine.step(), Err(EngineError::ActiveHardware));
    assert_eq!(device.read64(DDTP), 1);
    assert_eq!(engine.phase(), Phase::Prepared);
    drop(engine);
    assert!(pools.iter().all(|p| p.gives() == 1));
}

#[test]
fn partial_startup_keeps_pages_when_fault_queue_reports_failure() {
    let (mut engine, device, pools) = engine();
    assert_eq!(engine.step(), Ok(Phase::StartingFaults));
    device.set32(FQCSR, ENABLE | ON | (1 << 8));
    assert_eq!(engine.step(), Err(EngineError::Hardware));
    assert_eq!(engine.phase(), Phase::Failed);
    assert_eq!(device.read32(CQCSR), 0);
    drop(engine);
    assert!(pools.iter().all(|p| p.gives() == 0 && p.page().is_err()));
}

#[test]
fn wired_notifications_do_not_substitute_for_tagged_cq_completion() {
    use crate::iommu::riscv::registers::{CAP, FCTL, ICVEC, INTERRUPT, IPSR, PQCSR};
    let (mut engine, mut device, pools) = engine();
    assert_eq!(engine.enable_interrupts(), Err(EngineError::Unsupported));
    assert_eq!(device.read32(FCTL), 0);
    device.write64(CAP, device.read64(CAP) | (1 << 28));
    engine.enable_interrupts().unwrap();
    assert_eq!(device.read64(ICVEC), 0x3210);
    start(&mut engine);
    assert_eq!(device.read32(CQCSR) & INTERRUPT, INTERRUPT);
    assert_eq!(device.read32(FQCSR) & INTERRUPT, INTERRUPT);
    assert_eq!(device.read32(PQCSR), 0);
    let batch = engine.invalidate().unwrap();
    device.set32(IPSR, 1);
    assert_eq!(engine.observe(batch), Ok(false));
    device.complete(true, false);
    assert_eq!(engine.observe(batch), Ok(false));
    assert!(pools.iter().all(|p| p.gives() == 0));
    device.complete(true, true);
    assert_eq!(engine.observe(batch), Ok(true));
    assert!(pools.iter().all(|p| p.gives() == 0));
    engine.stop().unwrap();
    engine.step().unwrap();
    engine.step().unwrap();
    assert!(pools.iter().all(|p| p.gives() == 1));
}

fn topology() -> crate::topology::Topology {
    crate::routing_fixture::discover(
        crate::routing_fixture::root()
            .child(crate::routing_fixture::controller(
                crate::routing_fixture::CONTROLLER,
                1,
            ))
            .child(crate::routing_fixture::host(
                crate::routing_fixture::HOST,
                &[0, 1, 0, 0x10000],
            )),
    )
    .unwrap()
}
