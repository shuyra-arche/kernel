#![no_std]
#![no_main]
#![feature(allocator_ext, alloc_error_handler)]

extern crate alloc;

#[allow(dead_code)]
#[path = "../../../src/basis/cell/page/mod.rs"]
mod cell;
mod dma;
mod edu;
#[allow(dead_code)]
#[path = "../../support/iommu/mod.rs"]
pub(crate) mod iommu;
#[path = "../../../src/machine/iommu/riscv/mmio.rs"]
mod mmio;
#[path = "../../../src/entry/boot/devices/plan.rs"]
mod plan;
mod pool;
#[path = "../../../src/entry/boot/devices/ranges.rs"]
mod ranges;
#[path = "../../../src/entry/boot/devices/region.rs"]
mod region;
#[path = "../../../src/entry/boot/devices/reserved.rs"]
mod reserved;

mod basis {
    pub(crate) mod cell {
        pub(crate) use crate::cell as page;
    }
}

use ::core::fmt;
use ::core::fmt::Write;
use ::core::sync::atomic::Ordering;
use iommu::riscv::Engine;
use iommu::riscv::Phase;
use pool::POOL;
use pool::RETURNS;

::core::arch::global_asm!(
    ".section .text.entry",
    ".global _start",
    "_start:",
    "csrw mie, zero",
    ".option push",
    ".option norelax",
    "la gp, __global_pointer$",
    ".option pop",
    "la sp, _stack_top",
    "la t0, trap",
    "csrw mtvec, t0",
    "mv a0, a1",
    "tail guest",
);

struct Uart;
impl Write for Uart {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        for byte in value.bytes() {
            unsafe { (0x1000_0000 as *mut u8).write_volatile(byte) };
        }
        Ok(())
    }
}

fn finish(pass: bool) -> ! {
    unsafe { (0x10_0000 as *mut u32).write_volatile(if pass { 0x5555 } else { 0x13333 }) };
    loop {
        unsafe { ::core::arch::asm!("wfi") }
    }
}

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
    let fdt = unsafe { fdt::Fdt::from_ptr(dtb as *const u8) }.expect("DTB");
    let controller = fdt
        .all_nodes()
        .find(|node| {
            node.compatible()
                .is_some_and(|c| c.all().any(|s| s == "riscv,iommu"))
        })
        .expect("IOMMU missing");
    let region = controller.reg().unwrap().next().unwrap();
    assert_eq!(region.size, Some(4096));
    let base = region.starting_address.addr();
    if matches!(
        fdt.chosen().bootargs(),
        Some(
            "dma"
                | "dma-change"
                | "readonly"
                | "dma-abandon"
                | "owner-exit"
                | "activation-exit"
                | "route-reject"
        )
    ) {
        dma::run(base, fdt.chosen().bootargs().unwrap(), &fdt);
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
    if fdt.chosen().bootargs() == Some("abandon") {
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

mod machine {
    pub(crate) use crate::iommu;
}
