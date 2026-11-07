use crate::iommu::riscv::domain::{Domain, DomainPhase};
use crate::iommu::riscv::{Engine, Phase};
use crate::{
    Uart,
    edu::Edu,
    finish, mmio,
    pool::{LIMIT, POOL, RETURNS},
    progress,
};
use core::fmt::Write;
use core::sync::atomic::Ordering;

const IOVA: usize = 0x0040_0000;

pub fn run(base: usize, scenario: &str) -> ! {
    LIMIT.store(8, Ordering::Relaxed);
    let payload = POOL.cell().unwrap();
    let pa = payload.physical_address();
    let old_id = payload.id();
    let bytes = payload.as_mut_ptr();
    unsafe { bytes.write_bytes(0x5a, 4096) };
    let edu = Edu::new(1);
    let outsider = Edu::new(2);
    let cells = core::array::from_fn(|_| POOL.cell().unwrap());
    let mut engine = Engine::new(unsafe { mmio::Mmio::new(base) }, cells).unwrap();
    progress(&mut engine, Phase::Ready);
    let tables = core::array::from_fn(|_| POOL.cell().unwrap());
    // QEMU-only fixture: both plain emulated EDU functions are behind this
    // platform IOMMU, have no ATS/PRI capability, and no other hart or Unit runs.
    let mut domain =
        unsafe { Domain::new(engine, tables, payload, 8, IOVA, scenario != "readonly") }.unwrap();
    domain.start().unwrap();
    advance(&mut domain, DomainPhase::Active);
    edu.enable(1);
    outsider.enable(2);
    if scenario == "readonly" {
        edu.transfer(IOVA, true); // Cold translation: a forbidden write faults.
        fault(&mut domain, 8, 15, IOVA);
        check(bytes, 0x5a);
    }
    edu.transfer(IOVA, false); // Actual translated RAM -> endpoint DMA.
    assert!(domain.fault().unwrap().is_none(), "read translation failed");
    if scenario == "dma-abandon" {
        drop(domain);
        assert_eq!(RETURNS.load(Ordering::SeqCst), 0);
        assert!(POOL.cell().is_none());
        let _ = writeln!(Uart, "PASS iommu-dma abandoned=8 returns=0");
        finish(true);
    }
    unsafe { bytes.write_bytes(0xa5, 64) };
    edu.transfer(IOVA, true); // Endpoint -> same translated payload.
    if scenario == "readonly" {
        // QEMU 10's cached RO translation can deny the write in the generic
        // memory layer without another FQ record. Memory integrity is required
        // regardless; validate any records rather than depending on their count.
        for _ in 0..128 {
            let Some(fault) = domain.fault().unwrap() else {
                break;
            };
            assert_eq!((fault.device, fault.cause, fault.transaction), (8, 15, 3));
            assert!((IOVA as u64..IOVA as u64 + 64).contains(&fault.address));
        }
        check(bytes, 0xa5);
    } else {
        assert!(domain.fault().unwrap().is_none());
        check(bytes, 0x5a);
    }
    // Neither another DID nor a physical-address alias receives this Cell.
    outsider.transfer(IOVA, true);
    fault(&mut domain, 16, 258, IOVA);
    edu.transfer(pa, true);
    fault(&mut domain, 8, 15, pa);
    check(bytes, if scenario == "readonly" { 0xa5 } else { 0x5a });
    domain.revoke().unwrap();
    assert_eq!(RETURNS.load(Ordering::SeqCst), 0);
    assert!(POOL.cell().is_none());
    advance(&mut domain, DomainPhase::Blocked);
    // A future request using the old IOVA must fault after cache invalidation.
    edu.transfer(IOVA, true);
    fault(&mut domain, 8, 15, IOVA);
    check(bytes, if scenario == "readonly" { 0xa5 } else { 0x5a });
    assert_eq!(RETURNS.load(Ordering::SeqCst), 0);
    domain.close().unwrap();
    advance(&mut domain, DomainPhase::Closed);
    assert_eq!(RETURNS.load(Ordering::SeqCst), 3); // Only CQ/FQ/completion.
    let held: [_; 3] = core::array::from_fn(|_| POOL.cell().unwrap());
    assert!(POOL.cell().is_none()); // No data/table release from CQ/FQ evidence.
    // Trusted fixture performs PCI read completions after DDTP Off. EDU has no
    // ATS, QEMU's emulated DMA accesses are synchronous in its device callback,
    // and these reads order earlier requests. Future old-IOVA requests remain
    // blocked by DDTP Off. This is a platform-specific proof, not a driver Ack.
    edu.drain();
    outsider.drain();
    unsafe { domain.settle() }.unwrap();
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
    edu.transfer(IOVA, true);
    edu.drain();
    check(new_bytes, 0xc3); // Old DMA cannot corrupt the new Cell instance.
    drop((reused, held));
    let _ = if scenario == "readonly" {
        writeln!(Uart, "PASS iommu-dma readonly=1 denied=5")
    } else {
        writeln!(Uart, "PASS iommu-dma revoked=1 reused=1 denied=3")
    };
    finish(true)
}

fn check(bytes: *mut u8, expected: u8) {
    for i in 0..64 {
        assert_eq!(unsafe { bytes.add(i).read_volatile() }, expected);
    }
}

fn advance(domain: &mut Domain<mmio::Mmio>, target: DomainPhase) {
    for _ in 0..1_000_000 {
        if domain.step().unwrap() == target {
            return;
        }
    }
    panic!("domain deadline: {:?}", domain.phase());
}

fn fault(domain: &mut Domain<mmio::Mmio>, device: u32, cause: u16, address: usize) {
    for _ in 0..1_000_000 {
        if let Some(fault) = domain.fault().unwrap() {
            assert_eq!(fault.device, device);
            assert_eq!(fault.cause, cause);
            assert_eq!(fault.transaction, 3);
            assert_eq!(fault.address, address as u64);
            // QEMU may split a rejected 64-byte request into several faulting
            // transactions. Drain the bounded queue and validate every record.
            for _ in 0..128 {
                let Some(extra) = domain.fault().unwrap() else {
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
