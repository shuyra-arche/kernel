use super::{Console, lifecycle};
use core::{
    fmt::Write,
    sync::atomic::{AtomicUsize, Ordering},
};
use kernel_api::{
    address::VirtAddr,
    model::{
        exit::EXIT_FAULT,
        unit::{ExitCause, UnitQuery, UnitTarget},
    },
};
use kernel_client::call::{room, unit};
static STARTED: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "migration")]
static MIGRATION_ENTERED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
#[cfg(not(feature = "migration"))]
extern "C" fn probe() -> ! {
    let bytes: usize;
    unsafe { core::arch::asm!("csrr {}, vlenb", out(reg) bytes) };
    unit::exit(bytes as u64, 0, VirtAddr::new(0), 0)
}
#[cfg(not(feature = "migration"))]
extern "C" fn one() -> ! {
    worker(1, true)
}
#[cfg(not(feature = "migration"))]
extern "C" fn two() -> ! {
    worker(2, true)
}
fn worker(id: usize, barrier: bool) -> ! {
    let bytes: usize;
    unsafe { core::arch::asm!("csrr {}, vlenb", out(reg) bytes) };
    #[cfg(feature = "migration")]
    if !barrier {
        MIGRATION_ENTERED.store(true, Ordering::Release);
    }
    assert!(matches!(bytes, 16 | 32));
    let mut expected = [0u64; 128];
    let mut observed = [0u64; 128];
    let mut controls = [0usize; 4];
    unsafe { save(observed.as_mut_ptr(), controls.as_mut_ptr(), bytes) };
    assert_eq!(observed, [0; 128]);
    assert_eq!(controls, [0, 0, 1usize << 63, 0]);
    for (i, word) in expected[..bytes * 4].iter_mut().enumerate() {
        *word = (id as u64 * 0x1111111111111111) ^ (i as u64 * 0x1020304050607);
    }
    let wanted = [id, id + 3, 0, id]; // nonzero vstart, vl, e8/m1/tu/mu, vcsr
    unsafe { load(expected.as_ptr(), wanted.as_ptr(), bytes) };
    if barrier {
        STARTED.fetch_or(1 << id, Ordering::Release);
        let start: u64;
        unsafe { core::arch::asm!("rdtime {}", out(reg) start) };
        while STARTED.load(Ordering::Acquire) != 6 {
            let now: u64;
            unsafe { core::arch::asm!("rdtime {}", out(reg) now) };
            assert!(now - start < 30_000_000, "no vector timer preemption");
        }
    }
    for turn in 0..128 {
        unsafe {
            if id == 1 {
                context_vector_one();
            } else {
                context_vector_two();
            }
        }
        unsafe { save(observed.as_mut_ptr(), controls.as_mut_ptr(), bytes) };
        assert_eq!(&observed[..bytes * 4], &expected[..bytes * 4]);
        assert_eq!(controls, wanted);
        room::yield_();
        // Stagger both workers against each other and the Hart timers. A fixed
        // phase can keep a worker on one Hart for the entire checkpoint trace.
        let delay = 100_000 + ((turn * 17 + id * 11) % 29) as u64 * 100_000;
        room::sleep_until(room::clock() + delay).unwrap();
    }
    unsafe { save(observed.as_mut_ptr(), controls.as_mut_ptr(), bytes) };
    assert_eq!(&observed[..bytes * 4], &expected[..bytes * 4]);
    assert_eq!(controls, wanted);
    // A vector floating instruction must also lazily acquire FS, even after VS.
    unsafe {
        core::arch::asm!(
            ".option push",
            ".option arch, +v",
            "csrw vstart, zero",
            "vsetivli zero, 4, e32, m1, tu, mu",
            "vfadd.vv v0, v0, v0",
            ".option pop"
        );
    }
    unit::exit(0, 0, VirtAddr::new(0), 0)
}

#[cfg(not(feature = "migration"))]
pub(super) fn run() {
    let probe = lifecycle::spawn(probe as *const () as usize);
    unit::resume(UnitTarget::Task(probe)).unwrap();
    assert!(lifecycle::wait_task(probe));
    let info = lifecycle::inspect(UnitQuery::Target(UnitTarget::Task(probe)));
    lifecycle::delete(probe);
    if info.exit == Some((ExitCause::Fault, EXIT_FAULT as u64)) {
        writeln!(Console, "PASS context vector-absent=1").unwrap();
        return;
    }
    let bytes = info.result.unwrap() as usize;
    assert!(matches!(bytes, 16 | 32));
    let tasks = [
        lifecycle::spawn(one as *const () as usize),
        lifecycle::spawn(two as *const () as usize),
    ];
    for task in tasks {
        unit::resume(UnitTarget::Task(task)).unwrap();
    }
    for task in tasks {
        assert!(lifecycle::wait_task(task));
        assert_eq!(lifecycle::result(task), (1, 0));
        lifecycle::delete(task);
    }
    let message = if bytes == 16 {
        "PASS context vector=32 vlenb=16 controls=4 vstart=1 preempt=1 block=1\n"
    } else {
        "PASS context vector=32 vlenb=32 controls=4 vstart=1 preempt=1 block=1\n"
    };
    Console.write_str(message).unwrap();
}

#[unsafe(naked)]
unsafe extern "C" fn save(_data: *mut u64, _control: *mut usize, _bytes: usize) {
    core::arch::naked_asm!(
        ".option push",
        ".option arch, +v",
        "csrr t0, vstart",
        "sd t0, 0(a1)",
        "csrr t1, vl",
        "sd t1, 8(a1)",
        "csrr t1, vtype",
        "sd t1, 16(a1)",
        "csrr t1, vcsr",
        "sd t1, 24(a1)",
        "csrw vstart, zero",
        "slli a2, a2, 3",
        "vs8r.v v0, (a0)",
        "add a0, a0, a2",
        "vs8r.v v8, (a0)",
        "add a0, a0, a2",
        "vs8r.v v16, (a0)",
        "add a0, a0, a2",
        "vs8r.v v24, (a0)",
        "csrw vstart, t0",
        "ret",
        ".option pop",
    );
}
#[unsafe(naked)]
unsafe extern "C" fn load(_data: *const u64, _control: *const usize, _bytes: usize) {
    core::arch::naked_asm!(
        ".option push",
        ".option arch, +v",
        "csrw vstart, zero",
        "slli a2, a2, 3",
        "vl8re8.v v0, (a0)",
        "add a0, a0, a2",
        "vl8re8.v v8, (a0)",
        "add a0, a0, a2",
        "vl8re8.v v16, (a0)",
        "add a0, a0, a2",
        "vl8re8.v v24, (a0)",
        "ld t0, 8(a1)",
        "ld t1, 16(a1)",
        "vsetvl zero, t0, t1",
        "ld t0, 24(a1)",
        "csrw vcsr, t0",
        "ld t0, 0(a1)",
        "csrw vstart, t0",
        "ret",
        ".option pop",
    );
}

#[cfg(feature = "migration")]
extern "C" fn migrant() -> ! {
    worker(1, false)
}
#[cfg(feature = "migration")]
pub(super) fn migration() {
    // Keep every Hart runnable during the handoffs. Otherwise an idle Hart can
    // redeem a timer and steal the same Task back before the destination wakes,
    // which preserves state but does not exercise migration.
    let load = super::Load::start();
    for _ in 0..32 {
        MIGRATION_ENTERED.store(false, Ordering::Release);
        let task = lifecycle::spawn(migrant as *const () as usize);
        unit::resume(UnitTarget::Task(task)).unwrap();
        assert!(lifecycle::wait_task(task));
        let info = lifecycle::inspect(UnitQuery::Target(UnitTarget::Task(task)));
        lifecycle::delete(task);
        if info.exit == Some((ExitCause::Fault, EXIT_FAULT as u64)) {
            assert!(
                !MIGRATION_ENTERED.load(Ordering::Acquire),
                "fault after vector activation"
            );
            continue;
        }
        assert_eq!(info.result, Some(0));
        load.finish();
        Console
            .write_str("PASS context migration-state=1 incompatible-harts=2\n")
            .unwrap();
        return;
    }
    panic!("no vector-capable Hart admitted first use");
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
unsafe extern "C" fn context_vector_one() {
    core::arch::naked_asm!("nop", "ret");
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
unsafe extern "C" fn context_vector_two() {
    core::arch::naked_asm!("nop", "ret");
}
