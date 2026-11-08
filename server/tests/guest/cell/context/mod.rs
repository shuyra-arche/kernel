use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};
use kernel_api::{address::VirtAddr, model::unit::UnitTarget};
use kernel_client::call::{room, unit};
use super::{Console, lifecycle};
static STARTED: AtomicUsize = AtomicUsize::new(0);

#[repr(C, align(16))]
struct Image { regs: [u64; 32], fcsr: u64 }
impl Image { fn zero() -> Self { Self { regs: [0; 32], fcsr: 0 } } }

extern "C" fn one() -> ! { worker(1) }
extern "C" fn two() -> ! { worker(2) }
fn worker(id: usize) -> ! {
    let mut observed = Image::zero();
    // First use is a read, so a prior Task's hardware contents cannot hide.
    unsafe { save(&mut observed) };
    assert_eq!(observed.regs, [0; 32]);
    assert_eq!(observed.fcsr, 0);
    let mut expected = Image::zero();
    for (i, value) in expected.regs.iter_mut().enumerate() {
        *value = 0x4000_0000_0000_0000 | ((id as u64) << 40) | (i as u64 * 0x10203);
    }
    expected.fcsr = ((id as u64) << 5) | (id as u64 * 3);
    unsafe { load(&expected) };
    STARTED.fetch_or(1 << id, Ordering::Release);
    // With one Hart, progress here requires timer preemption; no Kernel call.
    let start: u64;
    unsafe { core::arch::asm!("rdtime {}", out(reg) start) };
    while STARTED.load(Ordering::Acquire) != 6 {
        let now: u64;
        unsafe { core::arch::asm!("rdtime {}", out(reg) now) };
        assert!(now - start < 30_000_000, "no timer preemption");
    }
    for _ in 0..16 {
        unsafe { save(&mut observed) };
        assert_eq!(observed.regs, expected.regs);
        assert_eq!(observed.fcsr, expected.fcsr);
        room::yield_();
        room::sleep_until(room::clock() + 1_000_000).unwrap();
    }
    unsafe { save(&mut observed) };
    assert_eq!(observed.regs, expected.regs);
    assert_eq!(observed.fcsr, expected.fcsr);
    unit::exit(0, 0, VirtAddr::new(0), 0)
}

pub(super) fn run() {
    let tasks = [lifecycle::spawn(one as *const () as usize), lifecycle::spawn(two as *const () as usize)];
    for task in tasks { unit::resume(UnitTarget::Task(task)).unwrap(); }
    for task in tasks {
        assert!(lifecycle::wait_task(task));
        assert_eq!(lifecycle::result(task), (1, 0));
        lifecycle::delete(task);
    }
    writeln!(Console, "PASS context float=32 fcsr=1 initial=1 preempt=1 block=1").unwrap();
}

#[unsafe(naked)]
unsafe extern "C" fn save(_image: *mut Image) {
    core::arch::naked_asm!(
        ".option push", ".option arch, +d",
        "frcsr t0", "sd t0, 256(a0)",
        "fsd f0, 0(a0)",
        "fsd f1, 8(a0)",
        "fsd f2, 16(a0)",
        "fsd f3, 24(a0)",
        "fsd f4, 32(a0)",
        "fsd f5, 40(a0)",
        "fsd f6, 48(a0)",
        "fsd f7, 56(a0)",
        "fsd f8, 64(a0)",
        "fsd f9, 72(a0)",
        "fsd f10, 80(a0)",
        "fsd f11, 88(a0)",
        "fsd f12, 96(a0)",
        "fsd f13, 104(a0)",
        "fsd f14, 112(a0)",
        "fsd f15, 120(a0)",
        "fsd f16, 128(a0)",
        "fsd f17, 136(a0)",
        "fsd f18, 144(a0)",
        "fsd f19, 152(a0)",
        "fsd f20, 160(a0)",
        "fsd f21, 168(a0)",
        "fsd f22, 176(a0)",
        "fsd f23, 184(a0)",
        "fsd f24, 192(a0)",
        "fsd f25, 200(a0)",
        "fsd f26, 208(a0)",
        "fsd f27, 216(a0)",
        "fsd f28, 224(a0)",
        "fsd f29, 232(a0)",
        "fsd f30, 240(a0)",
        "fsd f31, 248(a0)",
        "ret", ".option pop",
    );
}

#[unsafe(naked)]
unsafe extern "C" fn load(_image: *const Image) {
    core::arch::naked_asm!(
        ".option push", ".option arch, +d",
        "fld f0, 0(a0)",
        "fld f1, 8(a0)",
        "fld f2, 16(a0)",
        "fld f3, 24(a0)",
        "fld f4, 32(a0)",
        "fld f5, 40(a0)",
        "fld f6, 48(a0)",
        "fld f7, 56(a0)",
        "fld f8, 64(a0)",
        "fld f9, 72(a0)",
        "fld f10, 80(a0)",
        "fld f11, 88(a0)",
        "fld f12, 96(a0)",
        "fld f13, 104(a0)",
        "fld f14, 112(a0)",
        "fld f15, 120(a0)",
        "fld f16, 128(a0)",
        "fld f17, 136(a0)",
        "fld f18, 144(a0)",
        "fld f19, 152(a0)",
        "fld f20, 160(a0)",
        "fld f21, 168(a0)",
        "fld f22, 176(a0)",
        "fld f23, 184(a0)",
        "fld f24, 192(a0)",
        "fld f25, 200(a0)",
        "fld f26, 208(a0)",
        "fld f27, 216(a0)",
        "fld f28, 224(a0)",
        "fld f29, 232(a0)",
        "fld f30, 240(a0)",
        "fld f31, 248(a0)",
        "ld t0, 256(a0)", "fscsr t0",
        "ret", ".option pop",
    );
}
