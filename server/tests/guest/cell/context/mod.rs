mod boundary;
mod handoff;
#[cfg(feature = "vector")]
mod vector;
use super::{Console, lifecycle};
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use kernel_api::model::unit::ExitScope;
use kernel_api::{
    address::VirtAddr,
    model::{identity::TaskId, unit::UnitTarget, wait::Wait},
};
use kernel_client::call::{room, unit};
static STARTED: AtomicUsize = AtomicUsize::new(0);
static HANDOFF: handoff::Handoff = handoff::Handoff::new();

// Keep the four-Hart fixture runnable during context handoffs. Otherwise the
// timer owner can repeatedly steal an expired worker before an idle destination
// wakes: correct register preservation then provides no migration coverage.
static LOAD_STOP: AtomicBool = AtomicBool::new(false);
static LOAD_ENTERED: AtomicU32 = AtomicU32::new(0);
struct Load([TaskId; 4]);
impl Load {
    fn start() -> Self {
        LOAD_STOP.store(false, Ordering::Release);
        LOAD_ENTERED.store(0, Ordering::Release);
        let tasks = core::array::from_fn(|_| {
            let task = lifecycle::spawn(load_worker as *const () as usize);
            unit::resume(UnitTarget::Task(task)).unwrap();
            task
        });
        loop {
            let entered = LOAD_ENTERED.load(Ordering::Acquire);
            if entered == tasks.len() as u32 {
                break;
            }
            crate::room::wait(&LOAD_ENTERED, entered, Wait::Infinite);
        }
        Self(tasks)
    }
    fn finish(self) {
        LOAD_STOP.store(true, Ordering::Release);
        for task in self.0 {
            assert!(lifecycle::wait_task(task));
            assert_eq!(lifecycle::result(task), (1, 0));
            lifecycle::delete(task);
        }
    }
}
extern "C" fn load_worker() -> ! {
    LOAD_ENTERED.fetch_add(1, Ordering::Release);
    room::wake_value(VirtAddr::new(LOAD_ENTERED.as_ptr() as usize), true).unwrap();
    while !LOAD_STOP.load(Ordering::Acquire) {
        // Keep every Hart runnable and require timer preemption. A continuous
        // syscall loop instead benchmarks a lock convoy under oversubscribed
        // TCG, obscuring the register/placement transitions under test.
        core::hint::spin_loop();
    }
    unit::exit(0, 0, VirtAddr::new(0), 0, ExitScope::Task)
}

#[repr(C, align(16))]
struct Image {
    regs: [u64; 32],
    fcsr: u64,
}
impl Image {
    fn zero() -> Self {
        Self {
            regs: [0; 32],
            fcsr: 0,
        }
    }
}

extern "C" fn one() -> ! {
    worker(1)
}
extern "C" fn two() -> ! {
    worker(2)
}
fn worker(id: usize) -> ! {
    // Start the preemption experiment only after both Tasks exist and are
    // actually blocked. Creation/Resume of the second Task can itself take
    // time under debug contention; it is not part of the preemption bound.
    HANDOFF.wait(0);
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
    for turn in 0..8 {
        unsafe {
            if id == 1 {
                context_float_one();
            } else {
                context_float_two();
            }
        }
        unsafe { save(&mut observed) };
        assert_eq!(observed.regs, expected.regs);
        assert_eq!(observed.fcsr, expected.fcsr);
        HANDOFF.wait(turn + 1);
        // Every round covers a real blocked/resumed transition. Trace it
        // before Yield can move the Task, then check the whole register image.
        unsafe {
            if id == 1 {
                context_float_one();
            } else {
                context_float_two();
            }
            save(&mut observed);
        }
        assert_eq!(observed.regs, expected.regs);
        assert_eq!(observed.fcsr, expected.fcsr);
        room::yield_();
    }
    unsafe { save(&mut observed) };
    assert_eq!(observed.regs, expected.regs);
    assert_eq!(observed.fcsr, expected.fcsr);
    unit::exit(0, 0, VirtAddr::new(0), 0, ExitScope::Task)
}

pub(super) fn run() {
    #[cfg(all(feature = "vector", not(feature = "migration")))]
    vector::run();
    #[cfg(feature = "migration")]
    vector::migration();
    let load = Load::start();
    let tasks = [
        lifecycle::spawn(one as *const () as usize),
        lifecycle::spawn(two as *const () as usize),
    ];
    for task in tasks {
        unit::resume(UnitTarget::Task(task)).unwrap();
    }
    for turn in 0..9 {
        HANDOFF.wake(&tasks, turn);
    }
    for task in tasks {
        assert!(lifecycle::wait_task(task));
        assert_eq!(lifecycle::result(task), (1, 0));
        lifecycle::delete(task);
    }
    load.finish();
    boundary::run();
    writeln!(
        Console,
        "PASS context float=32 fcsr=1 initial=1 preempt=1 block=1"
    )
    .unwrap();
}

#[unsafe(naked)]
unsafe extern "C" fn save(_image: *mut Image) {
    core::arch::naked_asm!(
        ".option push",
        ".option arch, +d",
        "frcsr t0",
        "sd t0, 256(a0)",
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
        "ret",
        ".option pop",
    );
}

#[unsafe(naked)]
unsafe extern "C" fn load(_image: *const Image) {
    core::arch::naked_asm!(
        ".option push",
        ".option arch, +d",
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
        "ld t0, 256(a0)",
        "fscsr t0",
        "ret",
        ".option pop",
    );
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
unsafe extern "C" fn context_float_one() {
    core::arch::naked_asm!("nop", "ret");
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
unsafe extern "C" fn context_float_two() {
    core::arch::naked_asm!("nop", "ret");
}
