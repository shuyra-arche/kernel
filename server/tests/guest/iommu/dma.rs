use crate::Uart;
use crate::admission::Admission;
use crate::edu::Edu;
use crate::finish;
use crate::iommu::HardwareError;
use crate::iommu::IoSpace;
use crate::iommu::retirement::Registry;
use crate::iommu::riscv::Engine;
use crate::iommu::riscv::Phase;
use crate::iommu::riscv::adapter::Adapter;
use crate::iommu::contract::{Quiescence, Retirement};
use crate::iommu::riscv::controllers::Controllers;
use crate::iommu::riscv::topology::Topology;
use crate::mmio;
use crate::pool::LIMIT;
use crate::pool::POOL;
use crate::pool::RETURNS;
use crate::progress;
use alloc::sync::Arc;
use core::fmt::Write;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;

const IOVA: usize = 0x0040_0000;

// Trusted QEMU fixture topology, not a driver-supplied acknowledgement. This
// proof is intentionally not enabled on arbitrary physical PCI devices.
struct Platform<'a> {
    endpoints: [&'a Edu; 2],
    allow: &'a AtomicBool,
}
// SAFETY: plain emulated EDU has no ATS/PRI/bypass; all accesses pass this IOMMU.
// QEMU performs pci_dma accesses synchronously in its device callback. After
// DDTP Off, PCI read completions order older accesses; future delayed callbacks
// are denied. The test keeps DDTP Off and holds exclusive control of topology.
unsafe impl Quiescence for Platform<'_> {
    fn poll(&mut self, ticket: Retirement) -> Result<Option<Retirement>, HardwareError> {
        if !self.allow.load(Ordering::Acquire) {
            return Ok(None);
        }
        for endpoint in self.endpoints {
            endpoint.drain();
        }
        Ok(Some(ticket))
    }
}

type Space<'a> = IoSpace<Adapter<mmio::Mmio, Platform<'a>>>;

pub fn run(base: usize, scenario: &str, fdt: &fdt::Fdt) -> ! {
    let topology = Topology::discover(fdt).unwrap();
    // Exercise the production boot authority planner against QEMU's actual DTB,
    // including its zero-length tuple and PCI apertures, before touching devices.
    let plan = crate::plan::Plan::discover(fdt, 0x8000_0000, 128 << 20).unwrap();
    for authority in plan.authorities() {
        assert!(!authority.overlaps(crate::description::mmio::Region::new(base, 4096).unwrap()));
        for (ecam, size) in topology.configurations() {
            assert!(!authority.overlaps(crate::description::mmio::Region::new(ecam, size).unwrap()));
        }
    }
    let mut controllers = Controllers::new(topology).unwrap();
    LIMIT.store(8, Ordering::Relaxed);
    let payload = Arc::new(POOL.cell().unwrap());
    let pa = payload.physical_address();
    let old_id = payload.id();
    let bytes = payload.as_mut_ptr();
    unsafe { bytes.write_bytes(0x5a, 4096) };
    let edu = Edu::new(1);
    let outsider = Edu::new(2);
    let cells = core::array::from_fn(|_| POOL.cell().unwrap());
    let mut engine = Engine::new(unsafe { mmio::Mmio::new(base, base) }, cells).unwrap();
    progress(&mut engine, Phase::Ready);
    let tables = core::array::from_fn(|_| POOL.cell().unwrap());
    let allow = AtomicBool::new(false);
    let platform = Platform {
        endpoints: [&edu, &outsider],
        allow: &allow,
    };
    // Exclusive M-mode fixture, fixed coherent topology and whole-page borrowing.
    controllers
        .install(engine)
        .map_err(|(error, _)| error)
        .unwrap();
    if scenario == "route-reject" {
        let result = unsafe { controllers.bind(0x3000_1000, 8, tables, platform) };
        assert!(matches!(
            result,
            Err(crate::iommu::riscv::controllers::BindError::Route(
                crate::iommu::riscv::topology::RouteError::Unmapped
            ))
        ));
        assert_eq!(RETURNS.load(Ordering::SeqCst), 4); // unused tables returned
        let engine = controllers.engine(base).unwrap();
        assert_eq!(engine.phase(), Phase::Ready); // ownership was not consumed
        assert_eq!(unsafe { ((base + 0x10) as *const u64).read_volatile() }, 0);
        check(bytes, 0x5a);
        engine.stop().unwrap();
        progress(engine, Phase::Closed);
        drop(payload);
        assert_eq!(RETURNS.load(Ordering::SeqCst), 8);
        let _ = writeln!(Uart, "PASS iommu-binding rejected=1 returned=8");
        finish(true);
    }
    let adapter = unsafe { controllers.bind(0x3000_0000, 8, tables, platform) }.unwrap();
    let mut registry = Registry::new();
    let mut owner = Some(Admission::new().unwrap());
    let domain = registry
        .attach(owner.as_ref().unwrap(), adapter)
        .map_err(|(e, _)| e)
        .unwrap();
    let change = (scenario == "dma-change").then(|| payload.begin().unwrap());
    let freeze = change
        .as_ref()
        .map(|change| crate::cell::Freeze::new(change.clone()));
    let binding = registry
        .with_space(owner.as_ref().unwrap(), domain, |space| {
            let id = if let Some(change) = &change {
                assert!(unsafe { space.register_during(&payload, IOVA, true, None) }.is_err());
                unsafe { space.register_during(&payload, IOVA, true, Some(change)) }?
            } else {
                unsafe { space.register_during(&payload, IOVA, scenario != "readonly", None) }?
            };
            space.publish(id)?;
            Ok(id)
        })
        .unwrap();
    drop(freeze);
    drop(change);
    assert_eq!(Arc::strong_count(&payload), 2); // exactly one data obligation
    drop(payload);
    if scenario != "activation-exit" {
        let mut active = false;
        for _ in 0..1_000_000 {
            if registry
                .with_space(owner.as_ref().unwrap(), domain, |s| s.ready(binding))
                .unwrap()
            {
                active = true;
                break;
            }
        }
        assert!(active, "activation deadline");
        edu.enable(1);
        outsider.enable(2);
        if scenario == "readonly" {
            edu.transfer(IOVA, true);
            registry
                .with_space(owner.as_ref().unwrap(), domain, |s| {
                    fault(s, 8, 15, IOVA);
                    Ok(())
                })
                .unwrap();
            check(bytes, 0x5a);
        }
        edu.transfer(IOVA, false);
        registry
            .with_space(owner.as_ref().unwrap(), domain, |s| {
                assert!(s.fault()?.is_none(), "read translation failed");
                Ok(())
            })
            .unwrap();
        if scenario == "dma-abandon" {
            drop(registry);
            drop(owner);
            assert_eq!(RETURNS.load(Ordering::SeqCst), 0);
            assert!(POOL.cell().is_none());
            let _ = writeln!(Uart, "PASS iommu-dma abandoned=8 returns=0");
            finish(true);
        }
        unsafe { bytes.write_bytes(0xa5, 64) };
        edu.transfer(IOVA, true);
        registry
            .with_space(owner.as_ref().unwrap(), domain, |s| {
                if scenario == "readonly" {
                    // A cached RO denial need not produce another FQ record in QEMU.
                    for _ in 0..128 {
                        let Some(f) = s.fault()? else {
                            break;
                        };
                        assert_eq!((f.device, f.cause, f.transaction), (8, 15, 3));
                        assert!((IOVA as u64..IOVA as u64 + 64).contains(&f.address));
                    }
                    check(bytes, 0xa5);
                } else {
                    assert!(s.fault()?.is_none());
                    check(bytes, 0x5a);
                }
                outsider.transfer(IOVA, true);
                fault(s, 16, 258, IOVA);
                edu.transfer(pa, true);
                fault(s, 8, 15, pa);
                Ok(())
            })
            .unwrap();
    }
    let exiting = matches!(scenario, "owner-exit" | "activation-exit");
    if exiting {
        if scenario == "owner-exit" {
            edu.start(IOVA, true); // delayed DMA; requester disappears before it finishes
            assert_ne!(edu.read(0x98) & 1, 0, "DMA was not pending at exit");
        }
        drop(owner.take()); // no hardware work and no transfer of records
    } else {
        registry
            .with_space(owner.as_ref().unwrap(), domain, |s| s.revoke(binding))
            .unwrap();
    }
    assert_eq!(RETURNS.load(Ordering::SeqCst), 0);
    assert!(POOL.cell().is_none());
    for _ in 0..1_000_000 {
        if exiting {
            registry.step(1);
        } else {
            assert!(
                !registry
                    .with_space(owner.as_ref().unwrap(), domain, |s| s.poll(binding))
                    .unwrap()
            );
        }
        if RETURNS.load(Ordering::SeqCst) == 3 {
            break;
        }
    }
    assert_eq!(RETURNS.load(Ordering::SeqCst), 3); // CQ/FQ/completion only
    let held: [_; 3] = core::array::from_fn(|_| POOL.cell().unwrap());
    assert!(POOL.cell().is_none());
    // Many more complete queue observations cannot replace platform evidence.
    for _ in 0..32 {
        if exiting {
            registry.step(1);
        } else {
            assert!(
                !registry
                    .with_space(owner.as_ref().unwrap(), domain, |s| s.poll(binding))
                    .unwrap()
            );
        }
    }
    assert_eq!(registry.pending(), 1);
    assert_eq!(RETURNS.load(Ordering::SeqCst), 3);
    allow.store(true, Ordering::Release);
    if let Some(owner) = owner.take() {
        drop(owner);
    }
    for _ in 0..1_000_000 {
        registry.step(1);
        if registry.pending() == 0 {
            break;
        }
    }
    assert_eq!(registry.pending(), 0);
    assert_eq!(
        registry.error(domain),
        Err(crate::iommu::Error::UnknownDomain)
    );
    assert_eq!(RETURNS.load(Ordering::SeqCst), 8);
    let reused = POOL.cell().unwrap();
    assert_eq!(reused.physical_address(), pa);
    assert_ne!(reused.id(), old_id);
    let new_bytes = reused.as_mut_ptr();
    assert!(
        unsafe { core::slice::from_raw_parts(new_bytes, 4096) }
            .iter()
            .all(|b| *b == 0)
    );
    unsafe { new_bytes.write_bytes(0xc3, 64) };
    if scenario == "owner-exit" {
        edu.wait();
    } // late DMA cannot hit the reused Page
    edu.enable(1);
    edu.transfer(IOVA, true);
    edu.drain();
    check(new_bytes, 0xc3);
    drop((reused, held));
    let _ = match scenario {
        "dma-change" => writeln!(Uart, "PASS iommu-dma change=1 revoked=1 reused=1"),
        "readonly" => writeln!(Uart, "PASS iommu-dma readonly=1 denied=5"),
        "owner-exit" => writeln!(Uart, "PASS iommu-dma owner-exit=1 delayed-dma=1 reused=1"),
        "activation-exit" => writeln!(Uart, "PASS iommu-dma activation-exit=1 reused=1"),
        _ => writeln!(Uart, "PASS iommu-dma revoked=1 reused=1 denied=3"),
    };
    finish(true)
}

fn check(bytes: *mut u8, expected: u8) {
    for i in 0..64 {
        assert_eq!(unsafe { bytes.add(i).read_volatile() }, expected);
    }
}

fn fault(space: &mut Space<'_>, device: u32, cause: u16, address: usize) {
    for _ in 0..1_000_000 {
        if let Some(fault) = space.fault().unwrap() {
            assert_eq!(
                (fault.device, fault.cause, fault.transaction, fault.address),
                (device, cause, 3, address as u64)
            );
            for _ in 0..128 {
                let Some(extra) = space.fault().unwrap() else {
                    return;
                };
                assert_eq!(
                    (extra.device, extra.cause, extra.transaction),
                    (device, cause, 3)
                );
                assert!((address as u64..address as u64 + 64).contains(&extra.address));
            }
            panic!("fault queue did not drain");
        }
    }
    panic!("missing DMA fault");
}
