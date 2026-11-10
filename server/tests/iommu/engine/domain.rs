use super::CONTROLLER;
use super::engine;
use super::registers::Device;
use super::start;
use crate::cell::Exposure;
use crate::cell::Owner as PageOwner;
use crate::cell::Page;
use crate::iommu::EngineError;
use crate::iommu::Error;
use crate::iommu::HardwareError;
use crate::iommu::adapter::Adapter;
use crate::iommu::contract::{Quiescence, Retirement};
use crate::iommu::controllers::BindError;
use crate::iommu::controllers::Controllers;
use crate::iommu::registers::*;
use crate::publication::Phase;
use crate::support::Pool;
use crate::target::Target;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

type Space = Target<Adapter<Device, Proof>>;
#[derive(Default)]
struct Evidence {
    polls: usize,
    requests: Vec<Retirement>,
    receipts: VecDeque<Retirement>,
    outcomes: VecDeque<Result<bool, HardwareError>>,
}
struct Proof(Arc<Mutex<Evidence>>);
// No hardware DMA exists in the host fixture. The script only models the
// independent platform evidence boundary, never replaces production decisions.
unsafe impl Quiescence for Proof {
    fn prove(&mut self, ticket: Retirement) -> Result<Option<Retirement>, HardwareError> {
        let mut e = self.0.lock().unwrap();
        e.polls += 1;
        e.requests.push(ticket);
        if let Some(receipt) = e.receipts.pop_front() {
            return Ok(Some(receipt));
        }
        e.outcomes
            .pop_front()
            .unwrap_or(Ok(false))
            .map(|done| done.then_some(ticket))
    }
}

fn space(
    device_id: u32,
) -> (
    Space,
    Device,
    Vec<&'static Pool>,
    Arc<Page>,
    Arc<Mutex<Evidence>>,
) {
    let (mut engine, device, queues) = engine();
    start(&mut engine);
    let tables = core::array::from_fn::<_, 4, _>(|_| Pool::new(0));
    let payload = Pool::new(0);
    let cells = tables.map(|p| Page::from_internal(PageOwner::Kernel, p.page().unwrap()).unwrap());
    let cell = Arc::new(Page::from_internal(PageOwner::Kernel, payload.page().unwrap()).unwrap());
    let evidence = Arc::new(Mutex::new(Evidence::default()));
    let mut controllers = Controllers::new([CONTROLLER]).unwrap();
    controllers
        .install(engine)
        .map_err(|(error, _)| error)
        .unwrap();
    let adapter =
        unsafe { controllers.bind(CONTROLLER, device_id, cells, Proof(evidence.clone())) }.unwrap();
    (
        Target::new(adapter),
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

fn publish(space: &mut Space, cell: &Arc<Page>) {
    space.prepare(cell, 0x400000, true, None).unwrap();
    space.publish().unwrap();
    assert_eq!(space.phase(), Some(Phase::Publishing));
    // Domain never adds a second data retention.
    assert_eq!(Arc::strong_count(cell), 2);
}

fn active(space: &mut Space, device: &Device) {
    for _ in 0..20 {
        complete(device);
        if space.ready().unwrap() {
            assert_eq!(space.phase(), Some(Phase::Mapped));
            return;
        }
    }
    panic!("activation did not finish");
}

fn drain_to_platform(space: &mut Space, device: &Device, proof: &Arc<Mutex<Evidence>>) {
    for _ in 0..20 {
        complete(device);
        assert!(!space.poll().unwrap());
        if proof.lock().unwrap().polls > 0 {
            return;
        }
    }
    panic!("platform proof not reached");
}

fn finish(space: &mut Space, device: &Device, proof: &Arc<Mutex<Evidence>>) {
    proof.lock().unwrap().outcomes.extend([Ok(true); 8]);
    for _ in 0..40 {
        complete(device);
        if space.advance_close().unwrap() {
            return;
        }
    }
    panic!("binding close did not finish");
}

#[test]
fn translation_completion_cannot_discharge_platform_responsibility() {
    let (mut space, device, pools, cell, proof) = space(8);
    publish(&mut space, &cell);
    active(&mut space, &device);
    drop(cell);
    space.revoke().unwrap();
    drain_to_platform(&mut space, &device, &proof);
    assert_ne!(device.read64(DDTP), 0);
    assert!(pools[..3].iter().all(|p| p.gives() == 0));
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
    publish(&mut space, &cell);
    active(&mut space, &device);
    let old = space.revoke().unwrap().unwrap();
    drain_to_platform(&mut space, &device, &proof);
    proof
        .lock()
        .unwrap()
        .outcomes
        .push_back(Err(HardwareError::Timeout));
    assert_eq!(space.poll(), Err(Error::Hardware(HardwareError::Timeout)));
    assert_eq!(space.phase(), Some(Phase::Failed));
    assert_eq!(cell.exposure(), Exposure::External);
    assert!(!cell.allows_return());
    let new = space.revoke().unwrap().unwrap();
    assert_ne!(new, old);
    assert!(!space.poll().unwrap());
    proof.lock().unwrap().outcomes.push_back(Ok(true));
    assert!(space.poll().unwrap());
    assert_eq!(cell.exposure(), Exposure::Internal);
    assert_eq!(Arc::strong_count(&cell), 1);
    assert_eq!(pools[7].gives(), 0); // still held by the original owner
    drop(cell);
    finish(&mut space, &device, &proof);
    assert!(space.settled());
    assert!(pools[3..].iter().all(|p| p.gives() == 1));
}

#[test]
fn closing_during_each_activation_step_finishes_without_requester() {
    for steps in 0..4 {
        let (mut space, device, pools, cell, proof) = space(8);
        publish(&mut space, &cell);
        for _ in 0..steps {
            complete(&device);
            space.ready().unwrap();
        }
        drop(cell);
        proof.lock().unwrap().outcomes.extend([Ok(true); 8]);
        for _ in 0..30 {
            complete(&device);
            if space.advance_close().unwrap() {
                break;
            }
            assert_eq!(space.ready(), Err(Error::Closed));
        }
        assert!(space.settled());
        assert!(pools[3..].iter().all(|p| p.gives() == 1));
    }
}

#[test]
fn domain_failure_keeps_both_payload_and_hardware_tables() {
    let (mut space, device, pools, cell, proof) = space(8);
    publish(&mut space, &cell);
    active(&mut space, &device);
    drop(cell);
    space.revoke().unwrap();
    space.poll().unwrap(); // submit real withdrawal batch
    complete(&device);
    device.set32(CQCSR, ENABLE | ON | (1 << 9));
    assert_eq!(space.poll(), Err(Error::Hardware(HardwareError::Fault)));
    assert_eq!(proof.lock().unwrap().polls, 0);
    drop(space);
    assert!(pools.iter().all(|p| p.gives() == 0 && p.page().is_err()));
}

#[test]
fn invalid_geometry_and_extra_binding_fail_before_publication() {
    for (device_id, iova) in [(8, 1), (8, 1usize << 38)] {
        let (mut space, device, pools, cell, proof) = space(device_id);
        proof.lock().unwrap().outcomes.extend([Ok(true); 8]);
        assert!(space.prepare(&cell, iova, true, None).is_err());
        assert_eq!(space.phase(), None);
        assert_eq!(cell.exposure(), Exposure::Internal);
        assert_eq!(Arc::strong_count(&cell), 1);
        assert_eq!(device.read64(DDTP), 0);
        drop(cell);
        for _ in 0..5 {
            if space.advance_close().unwrap() {
                break;
            }
        }
        assert!(space.settled());
        assert!(pools[3..].iter().all(|p| p.gives() == 1));
    }
    let (mut space, device, pools, cell, proof) = space(8);
    proof.lock().unwrap().outcomes.extend([Ok(true); 8]);
    space.prepare(&cell, 0x400000, true, None).unwrap();
    assert_eq!(
        space.prepare(&cell, 0x500000, true, None),
        Err(Error::Overlap)
    );
    drop(cell);
    for _ in 0..5 {
        if space.advance_close().unwrap() {
            break;
        }
    }
    assert_eq!(device.read64(DDTP), 0);
    assert!(pools[3..].iter().all(|p| p.gives() == 1));
}

#[test]
fn rejected_bindings_preserve_controller_and_return_unpublished_tables() {
    let (mut engine, device, queues) = engine();
    start(&mut engine);
    let mut controllers = Controllers::new([CONTROLLER]).unwrap();
    controllers
        .install(engine)
        .map_err(|(error, _)| error)
        .unwrap();
    let evidence = Arc::new(Mutex::new(Evidence::default()));
    for (host, rid, expected) in [
        (CONTROLLER + 4096, 8, BindError::UnknownController),
        (CONTROLLER, 64, BindError::UnknownController),
    ] {
        let tables = core::array::from_fn::<_, 4, _>(|_| Pool::new(0));
        let cells =
            tables.map(|p| Page::from_internal(PageOwner::Kernel, p.page().unwrap()).unwrap());
        let result = unsafe { controllers.bind(host, rid, cells, Proof(evidence.clone())) };
        assert_eq!(result.err(), Some(expected));
        assert_eq!(device.read64(DDTP), 0);
        assert_eq!(
            controllers.engine(CONTROLLER).unwrap().phase(),
            crate::iommu::Phase::Ready
        );
        assert!(tables.iter().all(|p| p.gives() == 1));
        assert!(queues.iter().all(|p| p.gives() == 0));
    }
    let engine = controllers.engine(CONTROLLER).unwrap();
    engine.stop().unwrap();
    engine.step().unwrap();
    engine.step().unwrap();
    assert!(queues.iter().all(|p| p.gives() == 1));
}

#[test]
fn bound_controller_uses_admitted_device_id_and_cannot_be_assigned_twice() {
    let (mut engine, device, queues) = engine();
    start(&mut engine);
    let mut controllers = Controllers::new([CONTROLLER]).unwrap();
    controllers
        .install(engine)
        .map_err(|(error, _)| error)
        .unwrap();
    let tables = core::array::from_fn::<_, 4, _>(|_| Pool::new(0));
    let cells = tables.map(|p| Page::from_internal(PageOwner::Kernel, p.page().unwrap()).unwrap());
    let proof = Arc::new(Mutex::new(Evidence::default()));
    let adapter = unsafe { controllers.bind(CONTROLLER, 16, cells, Proof(proof.clone())) }.unwrap();
    let mut space = Target::new(adapter);
    let payload = Pool::new(0);
    let cell = Arc::new(Page::from_internal(PageOwner::Kernel, payload.page().unwrap()).unwrap());
    publish(&mut space, &cell);
    active(&mut space, &device);
    let directory = ((device.read64(DDTP) >> 10) << 12) as *const u64;
    assert_eq!(unsafe { directory.add(16 * 8).read_volatile() } & 1, 1);
    assert_eq!(unsafe { directory.add(8 * 8).read_volatile() }, 0);
    let extra = core::array::from_fn::<_, 4, _>(|_| Pool::new(0));
    let cells = extra.map(|p| Page::from_internal(PageOwner::Kernel, p.page().unwrap()).unwrap());
    assert_eq!(
        unsafe { controllers.bind(CONTROLLER, 17, cells, Proof(proof.clone())) }.err(),
        Some(BindError::Unavailable)
    );
    assert!(extra.iter().all(|p| p.gives() == 1));
    assert!(matches!(
        controllers.engine(CONTROLLER),
        Err(BindError::Unavailable)
    ));
    drop(cell);
    proof.lock().unwrap().outcomes.extend([Ok(true); 8]);
    for _ in 0..30 {
        complete(&device);
        if space.advance_close().unwrap() {
            break;
        }
    }
    assert!(space.settled());
    assert_eq!(payload.gives(), 1);
    assert!(tables.iter().all(|p| p.gives() == 1));
    assert!(queues.iter().all(|p| p.gives() == 0));
    assert!(matches!(
        controllers.engine(CONTROLLER),
        Err(BindError::Unavailable)
    ));
}

#[test]
fn physical_controller_identity_must_match_firmware_before_installation() {
    let mut controllers = Controllers::new([CONTROLLER]).unwrap();
    let mut device = Device::new();
    device.1 = 0x3020000;
    let pools = core::array::from_fn::<_, 3, _>(|_| Pool::new(0));
    let cells = pools.map(|p| Page::from_internal(PageOwner::Kernel, p.page().unwrap()).unwrap());
    let engine = crate::iommu::Engine::new(device.clone(), cells).unwrap();
    let (error, mut engine) = controllers
        .install(engine)
        .err()
        .expect("foreign MMIO rejected");
    assert_eq!(error, BindError::UnknownController);
    assert_eq!(device.read64(DDTP), 0);
    engine.stop().unwrap();
    assert!(pools[3..].iter().all(|p| p.gives() == 1));
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

#[test]
fn platform_receipt_is_bound_to_the_binding_and_retry_batch() {
    let (mut space, device, pools, cell, proof) = space(8);
    publish(&mut space, &cell);
    active(&mut space, &device);
    space.revoke().unwrap();
    drain_to_platform(&mut space, &device, &proof);
    let old = *proof.lock().unwrap().requests.last().unwrap();
    assert_eq!(old.controller(), CONTROLLER);
    assert_eq!(old.requester(), 8);
    proof
        .lock()
        .unwrap()
        .outcomes
        .push_back(Err(HardwareError::Timeout));
    assert!(space.poll().is_err());
    space.revoke().unwrap();
    proof.lock().unwrap().receipts.push_back(old);
    assert_eq!(space.poll(), Err(Error::Hardware(HardwareError::Fault)));
    assert_eq!(cell.exposure(), Exposure::External);
    assert!(pools[3..].iter().all(|p| p.gives() == 0));
    space.revoke().unwrap();
    proof.lock().unwrap().outcomes.push_back(Ok(true));
    assert!(space.poll().unwrap());
    assert_eq!(space.poll(), Err(Error::UnknownBinding));
    drop(cell);
    finish(&mut space, &device, &proof);
    assert!(pools[3..].iter().all(|p| p.gives() == 1));
}

#[test]
fn another_requesters_platform_receipt_cannot_settle_a_domain() {
    let (mut first, d1, p1, c1, q1) = space(8);
    let (mut second, d2, p2, c2, q2) = space(9);
    publish(&mut first, &c1);
    publish(&mut second, &c2);
    active(&mut first, &d1);
    active(&mut second, &d2);
    first.revoke().unwrap();
    second.revoke().unwrap();
    drain_to_platform(&mut first, &d1, &q1);
    drain_to_platform(&mut second, &d2, &q2);
    let foreign = *q1.lock().unwrap().requests.last().unwrap();
    q2.lock().unwrap().receipts.push_back(foreign);
    assert_eq!(second.poll(), Err(Error::Hardware(HardwareError::Fault)));
    assert_eq!(c2.exposure(), Exposure::External);
    assert!(p2[3..].iter().all(|p| p.gives() == 0));
    second.revoke().unwrap();
    for (space, proof) in [(&mut first, &q1), (&mut second, &q2)] {
        proof.lock().unwrap().outcomes.push_back(Ok(true));
        assert!(space.poll().unwrap());
    }
    drop((c1, c2));
    finish(&mut first, &d1, &q1);
    finish(&mut second, &d2, &q2);
    assert!(p1[3..].iter().chain(&p2[3..]).all(|p| p.gives() == 1));
}

#[test]
fn leaf_written_before_failed_submission_needs_its_own_withdrawal_fence() {
    use crate::iommu::contract::{BindingId, IoAddress};
    use crate::iommu::{BatchId, Hardware, Projection};
    use crate::publication::Publication;

    let (mut engine, device, _) = engine();
    start(&mut engine);
    let mut controllers = Controllers::new([CONTROLLER]).unwrap();
    controllers
        .install(engine)
        .map_err(|(error, _)| error)
        .unwrap();
    let tables = core::array::from_fn(|_| {
        Page::from_internal(PageOwner::Kernel, Pool::new(0).page().unwrap()).unwrap()
    });
    let leaf = tables[3].physical_address() as *const u64;
    let evidence = Arc::new(Mutex::new(Evidence::default()));
    let mut adapter =
        unsafe { controllers.bind(CONTROLLER, 8, tables, Proof(evidence.clone())) }.unwrap();
    let page =
        || Arc::new(Page::from_internal(PageOwner::Kernel, Pool::new(0).page().unwrap()).unwrap());
    let first = page();
    let mut receipt =
        Publication::prepare_external(&adapter, &first, 0x400000, true, None).unwrap();
    receipt.submit(&mut adapter).unwrap();
    for _ in 0..20 {
        complete(&device);
        if receipt.ready(&mut adapter).unwrap() {
            break;
        }
    }
    assert_eq!(receipt.phase(), Phase::Mapped);

    let second = page();
    let projection = Projection {
        id: BindingId::allocate().unwrap(),
        cell: second.id(),
        iova: IoAddress::page(0x401000).unwrap(),
        physical: second.physical_address(),
        size: second.size(),
        modifiable: true,
    };
    let mut keep = second.prepare_external(true).unwrap();
    keep.publish();
    let directory = device.read64(DDTP);
    let tail = device.read32(CQT);
    device.clone().write64(DDTP, directory | 16);
    assert_eq!(adapter.publish(projection), Err(HardwareError::Fault));
    assert_ne!(unsafe { leaf.add(1).read_volatile() } & 1, 0);
    assert_eq!(device.read32(CQT), tail);
    device.clone().write64(DDTP, directory);

    assert_eq!(adapter.ready(projection), Err(HardwareError::Fault));
    assert_eq!(adapter.capacity(), 0);
    let batch = BatchId::allocate().unwrap();
    adapter.revoke(projection).unwrap();
    adapter.fence(projection, batch).unwrap();
    assert_eq!(adapter.observe(batch), Ok(None));
    assert_eq!(unsafe { leaf.add(1).read_volatile() }, 0);
    assert_ne!(device.read32(CQT), tail);
    assert_eq!(adapter.observe(batch), Ok(None));
    assert!(evidence.lock().unwrap().requests.is_empty());
    assert_eq!(second.exposure(), Exposure::External);
    complete(&device);
    assert_eq!(adapter.observe(batch), Ok(None));
    assert_eq!(adapter.observe(batch), Ok(None));
    evidence.lock().unwrap().outcomes.push_back(Ok(true));
    assert_eq!(adapter.observe(batch), Ok(Some(batch)));
    unsafe { keep.retire().settle() };
    assert_eq!(second.exposure(), Exposure::Internal);
    assert_ne!(unsafe { leaf.read_volatile() } & 1, 0);

    receipt.revoke(&mut adapter).unwrap();
    evidence.lock().unwrap().outcomes.push_back(Ok(true));
    for _ in 0..20 {
        complete(&device);
        if receipt.poll(&mut adapter).unwrap() {
            break;
        }
    }
    assert_eq!(receipt.phase(), Phase::Settled);
    evidence.lock().unwrap().outcomes.push_back(Ok(true));
    for _ in 0..20 {
        complete(&device);
        if adapter.shutdown().unwrap() {
            break;
        }
    }
    assert!(adapter.take_engine().is_some());
}

#[test]
fn leaves_retire_independently_and_context_reuse_requires_binding_proof() {
    use crate::iommu::Hardware;
    use crate::iommu::contract::RetirementScope;
    use crate::publication::Publication;
    let (mut engine, device, queues) = engine();
    start(&mut engine);
    let mut controllers = Controllers::new([CONTROLLER]).unwrap();
    controllers
        .install(engine)
        .map_err(|(error, _)| error)
        .unwrap();
    let tables = core::array::from_fn::<_, 4, _>(|_| Pool::new(0));
    let cells = tables.map(|p| Page::from_internal(PageOwner::Kernel, p.page().unwrap()).unwrap());
    let evidence = Arc::new(Mutex::new(Evidence::default()));
    let leaf_address = cells[3].physical_address();
    let mut adapter =
        unsafe { controllers.bind(CONTROLLER, 8, cells, Proof(evidence.clone())) }.unwrap();
    let payloads = [Pool::new(0), Pool::new(0), Pool::new(0)];
    let pages = payloads
        .map(|p| Arc::new(Page::from_internal(PageOwner::Kernel, p.page().unwrap()).unwrap()));
    let prepare = |hardware: &Adapter<Device, Proof>, page: &Arc<Page>, address| {
        Publication::prepare_external(hardware, page, address, true, None).unwrap()
    };
    let mut first = prepare(&adapter, &pages[0], 0x400000);
    first.submit(&mut adapter).unwrap();
    for _ in 0..20 {
        complete(&device);
        if first.ready(&mut adapter).unwrap() {
            break;
        }
    }
    assert_eq!(first.phase(), Phase::Mapped);
    let mut second = prepare(&adapter, &pages[1], 0x401000);
    second.submit(&mut adapter).unwrap();
    for _ in 0..20 {
        complete(&device);
        if second.ready(&mut adapter).unwrap() {
            break;
        }
    }
    assert_eq!(second.phase(), Phase::Mapped);
    // Both entries actually exist in the retained page table, not merely in a ledger.
    let leaf = leaf_address as *const u64;
    assert_ne!(unsafe { leaf.read_volatile() } & 1, 0);
    let second_pte = unsafe { leaf.add(1).read_volatile() };
    assert_ne!(second_pte & 1, 0);
    let old = first.revoke(&mut adapter).unwrap().unwrap();
    for _ in 0..20 {
        complete(&device);
        assert!(!first.poll(&mut adapter).unwrap());
        if !evidence.lock().unwrap().requests.is_empty() {
            break;
        }
    }
    assert_eq!(unsafe { leaf.read_volatile() }, 0);
    assert_eq!(unsafe { leaf.add(1).read_volatile() }, second_pte);
    assert_ne!(device.read64(DDTP), 0);
    assert_eq!(pages[0].exposure(), Exposure::External);
    assert_eq!(
        evidence.lock().unwrap().requests.last().unwrap().scope(),
        RetirementScope::Range {
            address: 0x400000,
            size: 4096
        }
    );
    evidence.lock().unwrap().outcomes.push_back(Ok(true));
    assert!(first.poll(&mut adapter).unwrap());
    assert_eq!(pages[0].exposure(), Exposure::Internal);
    // Another Page can occupy the settled address; the former batch cannot act on it.
    let mut replacement = prepare(&adapter, &pages[2], 0x400000);
    replacement.submit(&mut adapter).unwrap();
    for _ in 0..20 {
        complete(&device);
        if replacement.ready(&mut adapter).unwrap() {
            break;
        }
    }
    assert_eq!(adapter.observe(old), Err(HardwareError::Fault));
    assert_eq!(unsafe { leaf.add(1).read_volatile() }, second_pte);
    assert!(Publication::prepare_external(&adapter, &pages[0], 0x600000, true, None).is_err());
    assert!(adapter.take_engine().is_none());
    assert!(!adapter.shutdown().unwrap());
    for receipt in [&mut second, &mut replacement] {
        receipt.revoke(&mut adapter).unwrap();
        evidence.lock().unwrap().outcomes.push_back(Ok(true));
        for _ in 0..20 {
            complete(&device);
            if receipt.poll(&mut adapter).unwrap() {
                break;
            }
        }
        assert_eq!(receipt.phase(), Phase::Settled);
    }
    // All payloads are safe but context tables and controller ownership are not.
    for _ in 0..20 {
        complete(&device);
        assert!(!adapter.shutdown().unwrap());
    }
    assert_eq!(device.read64(DDTP), 0);
    assert_eq!(
        evidence.lock().unwrap().requests.last().unwrap().scope(),
        RetirementScope::Binding
    );
    assert!(tables.iter().all(|p| p.gives() == 0));
    assert!(adapter.take_engine().is_none());
    evidence.lock().unwrap().outcomes.push_back(Ok(true));
    assert!(adapter.shutdown().unwrap());
    controllers.restore(adapter.take_engine().unwrap()).unwrap();
    assert!(tables.iter().all(|p| p.gives() == 1));
    assert!(queues.iter().all(|p| p.gives() == 0));
    let next_tables = core::array::from_fn::<_, 4, _>(|_| Pool::new(0));
    let next_cells =
        next_tables.map(|p| Page::from_internal(PageOwner::Kernel, p.page().unwrap()).unwrap());
    evidence.lock().unwrap().outcomes.push_back(Ok(true));
    let mut next = unsafe { controllers.bind(CONTROLLER, 8, next_cells, Proof(evidence)) }.unwrap();
    assert_eq!(next.observe(old), Err(HardwareError::Fault));
    assert!(next.shutdown().unwrap());
    controllers.restore(next.take_engine().unwrap()).unwrap();
    let engine = controllers.engine(CONTROLLER).unwrap();
    engine.stop().unwrap();
    for _ in 0..4 {
        engine.step().unwrap();
    }
    assert!(queues.iter().all(|p| p.gives() == 1));
}
