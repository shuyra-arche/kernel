use super::{engine, registers::Device, start};
use crate::iommu::riscv::EngineError;
use crate::iommu::riscv::adapter::{Adapter, Quiescence};
use crate::iommu::riscv::registers::*;
use crate::iommu::{BindingId, Error, HardwareError, IoSpace, Life, Phase};
use crate::{
    cell::{Cell, Exposure},
    support::Pool,
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

type Space = IoSpace<Adapter<Device, Proof>>;
#[derive(Default)]
struct Evidence {
    polls: usize,
    outcomes: VecDeque<Result<bool, HardwareError>>,
}
struct Proof(Arc<Mutex<Evidence>>);
// No hardware DMA exists in the host fixture. The script only models the
// independent platform evidence boundary, never replaces production decisions.
unsafe impl Quiescence for Proof {
    fn poll(&mut self) -> Result<bool, HardwareError> {
        let mut e = self.0.lock().unwrap();
        e.polls += 1;
        e.outcomes.pop_front().unwrap_or(Ok(false))
    }
}

fn space(
    device_id: u32,
) -> (
    Space,
    Device,
    Vec<&'static Pool>,
    Arc<Cell>,
    Arc<Mutex<Evidence>>,
) {
    let (mut engine, device, queues) = engine();
    start(&mut engine);
    let tables = core::array::from_fn::<_, 4, _>(|_| Pool::new(0));
    let payload = Pool::new(0);
    let cells = tables.map(|p| Cell::from_internal(p.page().unwrap()).unwrap());
    let cell = Arc::new(Cell::from_internal(payload.page().unwrap()).unwrap());
    let evidence = Arc::new(Mutex::new(Evidence::default()));
    let adapter = unsafe { Adapter::new(engine, cells, device_id, Proof(evidence.clone())) }
        .map_err(|(error, _)| error)
        .unwrap();
    (
        IoSpace::new(adapter),
        device,
        queues.into_iter().chain(tables).chain([payload]).collect(),
        cell,
        evidence,
    )
}

fn complete(device: &Device) {
    if device.read32(CQH) != device.read32(CQT) {
        device.complete(true, true);
    }
}

fn publish(space: &mut Space, cell: &Arc<Cell>) -> BindingId {
    let id = unsafe { space.register(cell, 0x400000, true) }.unwrap();
    space.publish(id).unwrap();
    assert_eq!(space.phase(id), Ok(Phase::Publishing));
    // Domain never adds a second data retention.
    assert_eq!(Arc::strong_count(cell), 2);
    id
}

fn active(space: &mut Space, device: &Device, id: BindingId) {
    for _ in 0..20 {
        complete(device);
        if space.ready(id).unwrap() {
            assert_eq!(space.phase(id), Ok(Phase::Active));
            return;
        }
    }
    panic!("activation did not finish");
}

fn drain_to_platform(
    space: &mut Space,
    device: &Device,
    proof: &Arc<Mutex<Evidence>>,
    id: BindingId,
) {
    for _ in 0..20 {
        complete(device);
        assert!(!space.poll(id).unwrap());
        if proof.lock().unwrap().polls > 0 {
            return;
        }
    }
    panic!("platform proof not reached");
}

#[test]
fn translation_completion_cannot_discharge_platform_responsibility() {
    let (mut space, device, pools, cell, proof) = space(8);
    let id = publish(&mut space, &cell);
    active(&mut space, &device, id);
    drop(cell);
    space.revoke(id).unwrap();
    drain_to_platform(&mut space, &device, &proof, id);
    assert_eq!(device.read64(DDTP), 0);
    assert!(pools[..3].iter().all(|p| p.gives() == 1));
    assert!(pools[3..].iter().all(|p| p.gives() == 0));
    drop(space);
    assert!(
        pools[3..]
            .iter()
            .all(|p| p.gives() == 0 && p.page().is_err())
    );
}

#[test]
fn platform_failure_retains_data_and_retry_requires_fresh_evidence() {
    let (mut space, device, pools, cell, proof) = space(8);
    let id = publish(&mut space, &cell);
    active(&mut space, &device, id);
    let old = space.revoke(id).unwrap().unwrap();
    drain_to_platform(&mut space, &device, &proof, id);
    proof
        .lock()
        .unwrap()
        .outcomes
        .push_back(Err(HardwareError::Timeout));
    assert_eq!(space.poll(id), Err(Error::Hardware(HardwareError::Timeout)));
    assert_eq!(space.phase(id), Ok(Phase::Failed));
    assert_eq!(cell.exposure(), Exposure::External);
    assert!(!cell.allows_return());
    let new = space.revoke(id).unwrap().unwrap();
    assert_ne!(new, old);
    assert!(!space.poll(id).unwrap());
    proof.lock().unwrap().outcomes.push_back(Ok(true));
    assert!(space.poll(id).unwrap());
    assert_eq!(cell.exposure(), Exposure::Internal);
    assert_eq!(Arc::strong_count(&cell), 1);
    assert_eq!(pools[7].gives(), 0); // still held by the original owner
    drop(cell);
    space.close().unwrap();
    assert_eq!(space.life(), Life::Closed);
    assert!(pools.iter().all(|p| p.gives() == 1));
}

#[test]
fn closing_during_each_activation_step_finishes_without_requester() {
    for steps in 0..4 {
        let (mut space, device, pools, cell, proof) = space(8);
        let id = publish(&mut space, &cell);
        for _ in 0..steps {
            complete(&device);
            space.ready(id).unwrap();
        }
        drop(cell);
        proof.lock().unwrap().outcomes.push_back(Ok(true));
        for _ in 0..30 {
            complete(&device);
            if space.advance_close().unwrap() {
                break;
            }
            assert_eq!(space.ready(id), Err(Error::Closed));
        }
        assert_eq!(space.life(), Life::Closed);
        assert!(pools.iter().all(|p| p.gives() == 1));
    }
}

#[test]
fn domain_failure_keeps_both_payload_and_hardware_tables() {
    let (mut space, device, pools, cell, proof) = space(8);
    let id = publish(&mut space, &cell);
    active(&mut space, &device, id);
    drop(cell);
    space.revoke(id).unwrap();
    space.poll(id).unwrap(); // submit real withdrawal batch
    complete(&device);
    device.set32(CQCSR, ENABLE | ON | (1 << 9));
    assert_eq!(space.poll(id), Err(Error::Hardware(HardwareError::Fault)));
    assert_eq!(proof.lock().unwrap().polls, 0);
    drop(space);
    assert!(pools.iter().all(|p| p.gives() == 0 && p.page().is_err()));
}

#[test]
fn invalid_geometry_and_extra_binding_fail_before_publication() {
    for (device_id, iova) in [(64, 0x400000), (8, 1), (8, 1usize << 38)] {
        let (mut space, device, pools, cell, _) = space(device_id);
        assert!(unsafe { space.register(&cell, iova, true) }.is_err());
        assert_eq!(space.pending(), 0);
        assert_eq!(cell.exposure(), Exposure::Internal);
        assert_eq!(Arc::strong_count(&cell), 1);
        assert_eq!(device.read64(DDTP), 0);
        drop(cell);
        for _ in 0..5 {
            if space.advance_close().unwrap() {
                break;
            }
        }
        assert_eq!(space.life(), Life::Closed);
        assert!(pools.iter().all(|p| p.gives() == 1));
    }
    let (mut space, device, pools, cell, _) = space(8);
    unsafe { space.register(&cell, 0x400000, true) }.unwrap();
    assert_eq!(
        unsafe { space.register(&cell, 0x500000, true) },
        Err(Error::Capacity)
    );
    drop(cell);
    for _ in 0..5 {
        if space.advance_close().unwrap() {
            break;
        }
    }
    assert_eq!(device.read64(DDTP), 0);
    assert!(pools.iter().all(|p| p.gives() == 1));
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
