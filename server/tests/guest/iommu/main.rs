#![no_std]
#![no_main]
#![feature(allocator_ext, alloc_error_handler)]

extern crate alloc;

#[allow(dead_code, unused_imports)]
#[path = "../../../src/basis/cell/page/mod.rs"]
mod cell;
mod dma;
mod lock;
use kernel_hardware_fixture::{Edu, Uart, finish};
#[allow(dead_code)]
#[path = "../../support/iommu/mod.rs"]
pub(crate) mod iommu;
#[path = "../../../src/metal/riscv/iommu/mmio.rs"]
mod mmio;
mod pool;

// This hardware-only harness needs serial identities, not registration tables.
#[path = "../../../src/basis/name/serial.rs"]
mod serial;
mod name {
    pub(crate) use crate::serial::Serial;
}

#[allow(dead_code)]
#[path = "../../../src/basis/nest/translation/external/state.rs"]
pub(crate) mod external;
#[allow(dead_code)]
#[path = "../../../src/basis/nest/memo/publication/receipt.rs"]
pub(crate) mod publication;
#[allow(dead_code)]
#[path = "../../support/iommu/target.rs"]
pub(crate) mod target;

mod basis {
    pub(crate) use crate::name;
    pub(crate) mod nest {
        pub(crate) use crate::iommu::Error as PublicationError;
        pub(crate) mod memo {
            pub(crate) use crate::publication;
        }
    }
    pub(crate) mod cell {
        pub(crate) use crate::cell as page;
    }
}

use ::core::fmt::Write;
use ::core::sync::atomic::Ordering;
use iommu::Engine;
use iommu::Phase;
use pool::POOL;
use pool::RETURNS;

#[unsafe(no_mangle)]
extern "C" fn trap() -> ! {
    let (cause, pc, value): (usize, usize, usize);
    unsafe {
        ::core::arch::asm!("csrr {}, mcause", "csrr {}, mepc", "csrr {}, mtval",
        out(reg) cause, out(reg) pc, out(reg) value)
    };
    let _ = writeln!(Uart, "FAIL trap cause={cause:x} pc={pc:x} value={value:x}");
    finish(false)
}

#[panic_handler]
fn panic(info: &::core::panic::PanicInfo) -> ! {
    let _ = writeln!(Uart, "FAIL {info}");
    finish(false)
}

#[alloc_error_handler]
fn allocation(_: ::core::alloc::Layout) -> ! {
    panic!("guest heap exhausted")
}

#[unsafe(no_mangle)]
extern "C" fn guest(dtb: usize) -> ! {
    let environment = unsafe { kernel_hardware_fixture::discover(dtb) };
    let base = environment.controller;
    if matches!(
        environment.scenario,
        "dma"
            | "dma-change"
            | "readonly"
            | "dma-abandon"
            | "owner-exit"
            | "activation-exit"
            | "route-reject"
    ) {
        dma::run(base, environment.scenario, environment.requester);
    }
    let _ = writeln!(Uart, "BEGIN iommu-command-engine base={base:#x}");
    let cells = [
        POOL.cell().unwrap(),
        POOL.cell().unwrap(),
        POOL.cell().unwrap(),
    ];
    let first_pa = cells[0].physical_address();
    let first_id = cells[0].id();
    // QEMU virt's platform IOMMU provides coherent access. This guest owns the
    // full machine, never enables device domains and keeps other harts absent.
    let mut engine = Engine::new(unsafe { mmio::Mmio::new(base, base) }, cells).unwrap();
    progress(&mut engine, Phase::Ready);
    assert!(POOL.cell().is_none());
    if environment.scenario == "abandon" {
        let _batch = engine.invalidate().unwrap();
        drop(engine);
        assert_eq!(RETURNS.load(Ordering::SeqCst), 0);
        assert!(POOL.cell().is_none());
        let _ = writeln!(Uart, "PASS iommu-command-engine abandoned=3 returns=0");
        finish(true)
    }
    for _ in 0..300 {
        let batch = engine.invalidate().unwrap();
        assert!(engine.stop().is_err());
        assert_eq!(RETURNS.load(Ordering::SeqCst), 0);
        assert!(POOL.cell().is_none());
        let mut done = false;
        for _ in 0..1_000_000 {
            if engine.observe(batch).unwrap() {
                done = true;
                break;
            }
        }
        assert!(done, "fence deadline");
    }
    engine.stop().unwrap();
    assert_eq!(RETURNS.load(Ordering::SeqCst), 0);
    progress(&mut engine, Phase::Closed);
    assert_eq!(RETURNS.load(Ordering::SeqCst), 3);
    let reused = POOL.cell().unwrap();
    assert_eq!(reused.physical_address(), first_pa);
    assert_ne!(reused.id(), first_id);
    assert!(
        unsafe { ::core::slice::from_raw_parts(reused.as_mut_ptr(), 4096) }
            .iter()
            .all(|byte| *byte == 0)
    );
    drop(reused);
    assert_eq!(RETURNS.load(Ordering::SeqCst), 4);
    let _ = writeln!(Uart, "PASS iommu-command-engine batches=300 returns=4");
    finish(true)
}

fn progress(engine: &mut Engine<mmio::Mmio>, target: Phase) {
    for _ in 0..1_000_000 {
        if engine.step().unwrap() == target {
            return;
        }
    }
    panic!("engine deadline: {:?}", engine.phase());
}

mod metal {
    pub(crate) use crate::lock as sync;
    pub(crate) mod generic {
        pub(crate) use crate::iommu::contract as iommu;
    }
    pub(crate) use crate::iommu;
}
