use super::engine;
use super::registers::Device;
use super::start;
use crate::cell::Exposure;
use crate::cell::Owner as PageOwner;
use crate::cell::Page;
use crate::iommu::BindingId;
use crate::iommu::Error;
use crate::iommu::HardwareError;
use crate::iommu::IoSpace;
use crate::iommu::Life;
use crate::iommu::Phase;
use crate::iommu::contract::{Quiescence, Retirement};
use crate::iommu::riscv::EngineError;
use crate::iommu::riscv::adapter::Adapter;
use crate::iommu::riscv::controllers::BindError;
use crate::iommu::riscv::controllers::Controllers;
use crate::iommu::riscv::registers::*;
use crate::routing_fixture;
use crate::support::Pool;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

type Space = IoSpace<Adapter<Device, Proof>>;
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
    fn poll(&mut self, ticket: Retirement) -> Result<Option<Retirement>, HardwareError> {
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
    let mut controllers = Controllers::new(super::topology()).unwrap();
    controllers
        .install(engine)
        .map_err(|(error, _)| error)
        .unwrap();
    let adapter = unsafe {
        controllers.bind(
            routing_fixture::HOST,
            device_id as u16,
            cells,
            Proof(evidence.clone()),
        )
    }
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

fn publish(space: &mut Space, cell: &Arc<Page>) -> BindingId {
    let id = unsafe { space.register_during(cell, 0x400000, true, None) }.unwrap();
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
    for (device_id, iova) in [(8, 1), (8, 1usize << 38)] {
        let (mut space, device, pools, cell, _) = space(device_id);
        assert!(unsafe { space.register_during(&cell, iova, true, None) }.is_err());
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
    unsafe { space.register_during(&cell, 0x400000, true, None) }.unwrap();
    assert_eq!(
        unsafe { space.register_during(&cell, 0x500000, true, None) },
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
fn rejected_bindings_preserve_controller_and_return_unpublished_tables() {
    use crate::iommu::riscv::topology::RouteError;
    let (mut engine, device, queues) = engine();
    start(&mut engine);
    let mut controllers = Controllers::new(super::topology()).unwrap();
    controllers
        .install(engine)
        .map_err(|(error, _)| error)
        .unwrap();
    let evidence = Arc::new(Mutex::new(Evidence::default()));
    for (host, rid, expected) in [
        (
            routing_fixture::HOST + 4096,
            8,
            BindError::Route(RouteError::Unmapped),
        ),
        (
            routing_fixture::HOST,
            64,
            BindError::Engine(EngineError::Address),
        ),
    ] {
        let tables = core::array::from_fn::<_, 4, _>(|_| Pool::new(0));
        let cells =
            tables.map(|p| Page::from_internal(PageOwner::Kernel, p.page().unwrap()).unwrap());
        let result = unsafe { controllers.bind(host, rid, cells, Proof(evidence.clone())) };
        assert_eq!(result.err(), Some(expected));
        assert_eq!(device.read64(DDTP), 0);
        assert_eq!(
            controllers
                .engine(routing_fixture::CONTROLLER)
                .unwrap()
                .phase(),
            crate::iommu::riscv::Phase::Ready
        );
        assert!(tables.iter().all(|p| p.gives() == 1));
        assert!(queues.iter().all(|p| p.gives() == 0));
    }
    let engine = controllers.engine(routing_fixture::CONTROLLER).unwrap();
    engine.stop().unwrap();
    engine.step().unwrap();
    engine.step().unwrap();
    assert!(queues.iter().all(|p| p.gives() == 1));
}

#[test]
fn bound_controller_uses_translated_device_id_and_cannot_be_assigned_twice() {
    use routing_fixture::*;
    let (mut engine, device, queues) = engine();
    start(&mut engine);
    let topology = discover(
        root()
            .child(controller(CONTROLLER, 1))
            .child(host(HOST, &[8, 1, 16, 8])),
    )
    .unwrap();
    let mut controllers = Controllers::new(topology).unwrap();
    controllers
        .install(engine)
        .map_err(|(error, _)| error)
        .unwrap();
    let tables = core::array::from_fn::<_, 4, _>(|_| Pool::new(0));
    let cells = tables.map(|p| Page::from_internal(PageOwner::Kernel, p.page().unwrap()).unwrap());
    let proof = Arc::new(Mutex::new(Evidence::default()));
    let adapter = unsafe { controllers.bind(HOST, 8, cells, Proof(proof.clone())) }.unwrap();
    let mut space = IoSpace::new(adapter);
    let payload = Pool::new(0);
    let cell = Arc::new(Page::from_internal(PageOwner::Kernel, payload.page().unwrap()).unwrap());
    let id = publish(&mut space, &cell);
    active(&mut space, &device, id);
    let directory = ((device.read64(DDTP) >> 10) << 12) as *const u64;
    assert_eq!(unsafe { directory.add(16 * 8).read_volatile() } & 1, 1);
    assert_eq!(unsafe { directory.add(8 * 8).read_volatile() }, 0);
    let extra = core::array::from_fn::<_, 4, _>(|_| Pool::new(0));
    let cells = extra.map(|p| Page::from_internal(PageOwner::Kernel, p.page().unwrap()).unwrap());
    assert_eq!(
        unsafe { controllers.bind(HOST, 9, cells, Proof(proof.clone())) }.err(),
        Some(BindError::Unavailable)
    );
    assert!(extra.iter().all(|p| p.gives() == 1));
    assert!(matches!(
        controllers.engine(CONTROLLER),
        Err(BindError::Unavailable)
    ));
    drop(cell);
    proof.lock().unwrap().outcomes.push_back(Ok(true));
    for _ in 0..30 {
        complete(&device);
        if space.advance_close().unwrap() {
            break;
        }
    }
    assert_eq!(space.life(), Life::Closed);
    assert_eq!(payload.gives(), 1);
    assert!(tables.iter().chain(queues.iter()).all(|p| p.gives() == 1));
    assert!(matches!(
        controllers.engine(CONTROLLER),
        Err(BindError::Unavailable)
    ));
}

#[test]
fn physical_controller_identity_must_match_firmware_before_installation() {
    let mut controllers = Controllers::new(super::topology()).unwrap();
    let mut device = Device::new();
    device.1 = 0x3020000;
    let pools = core::array::from_fn::<_, 3, _>(|_| Pool::new(0));
    let cells = pools.map(|p| Page::from_internal(PageOwner::Kernel, p.page().unwrap()).unwrap());
    let engine = crate::iommu::riscv::Engine::new(device.clone(), cells).unwrap();
    let (error, mut engine) = controllers
        .install(engine)
        .err()
        .expect("foreign MMIO rejected");
    assert_eq!(error, BindError::UnknownController);
    assert_eq!(device.read64(DDTP), 0);
    engine.stop().unwrap();
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

#[test]
fn platform_receipt_is_bound_to_the_binding_and_retry_batch() {
    let (mut space, device, pools, cell, proof) = space(8);
    let id = publish(&mut space, &cell);
    active(&mut space, &device, id);
    space.revoke(id).unwrap();
    drain_to_platform(&mut space, &device, &proof, id);
    let old = *proof.lock().unwrap().requests.last().unwrap();
    assert_eq!(old.controller(), routing_fixture::CONTROLLER);
    assert_eq!(old.requester(), 8);
    proof
        .lock()
        .unwrap()
        .outcomes
        .push_back(Err(HardwareError::Timeout));
    assert!(space.poll(id).is_err());
    space.revoke(id).unwrap();
    proof.lock().unwrap().receipts.push_back(old);
    assert_eq!(space.poll(id), Err(Error::Hardware(HardwareError::Fault)));
    assert_eq!(cell.exposure(), Exposure::External);
    assert!(pools[3..].iter().all(|p| p.gives() == 0));
    space.revoke(id).unwrap();
    proof.lock().unwrap().outcomes.push_back(Ok(true));
    assert!(space.poll(id).unwrap());
    assert_eq!(space.poll(id), Err(Error::UnknownBinding));
    drop(cell);
    assert!(pools.iter().all(|p| p.gives() == 1));
}

#[test]
fn another_requesters_platform_receipt_cannot_settle_a_domain() {
    let (mut first, d1, p1, c1, q1) = space(8);
    let (mut second, d2, p2, c2, q2) = space(9);
    let a = publish(&mut first, &c1);
    let b = publish(&mut second, &c2);
    active(&mut first, &d1, a);
    active(&mut second, &d2, b);
    first.revoke(a).unwrap();
    second.revoke(b).unwrap();
    drain_to_platform(&mut first, &d1, &q1, a);
    drain_to_platform(&mut second, &d2, &q2, b);
    let foreign = *q1.lock().unwrap().requests.last().unwrap();
    q2.lock().unwrap().receipts.push_back(foreign);
    assert_eq!(second.poll(b), Err(Error::Hardware(HardwareError::Fault)));
    assert_eq!(c2.exposure(), Exposure::External);
    assert!(p2[3..].iter().all(|p| p.gives() == 0));
    second.revoke(b).unwrap();
    for (space, id, proof) in [(&mut first, a, q1), (&mut second, b, q2)] {
        proof.lock().unwrap().outcomes.push_back(Ok(true));
        assert!(space.poll(id).unwrap());
    }
    drop((c1, c2));
    assert!(p1.iter().chain(&p2).all(|p| p.gives() == 1));
}
