use super::{Console, Image, lifecycle, load, save};
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};
use kernel_api::model::unit::ExitScope;
use kernel_api::{
    address::VirtAddr,
    model::{
        exit::EXIT_FAULT,
        unit::{ExitCause, UnitQuery, UnitState, UnitTarget},
    },
};
use kernel_client::call::{room, unit};
static STARTED: AtomicBool = AtomicBool::new(false);
extern "C" fn invalid() -> ! {
    unsafe {
        core::arch::asm!(".4byte 0x30002573");
    } // U-mode read of mstatus
    unit::exit(1, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
extern "C" fn active_invalid() -> ! {
    let expected = Image {
        regs: [0x12345678; 32],
        fcsr: 31,
    };
    unsafe {
        load(&expected);
        core::arch::asm!(".4byte 0x0000000b");
    }
    unit::exit(1, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
extern "C" fn fresh() -> ! {
    let mut image = Image::zero();
    unsafe { save(&mut image) };
    assert_eq!(image.regs, [0; 32]);
    assert_eq!(image.fcsr, 0);
    unit::exit(0, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
extern "C" fn running() -> ! {
    unsafe {
        load(&Image {
            regs: [0x987654321; 32],
            fcsr: 21,
        })
    };
    STARTED.store(true, Ordering::Release);
    loop {
        core::hint::spin_loop();
    }
}
extern "C" fn blocked() -> ! {
    unsafe {
        load(&Image {
            regs: [0x123456789; 32],
            fcsr: 10,
        })
    };
    STARTED.store(true, Ordering::Release);
    room::sleep_until(room::clock() + 10_000_000_000).unwrap();
    unit::exit(1, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
#[unsafe(naked)]
extern "C" fn first_single() -> ! {
    core::arch::naked_asm!("fmv.w.x f0, zero", "fmv.x.w a0, f0", "frcsr a1", "tail {done}", done = sym first_done);
}
#[unsafe(naked)]
extern "C" fn first_double() -> ! {
    core::arch::naked_asm!("fmv.d.x f0, zero", "fmv.x.d a0, f0", "frcsr a1", "tail {done}", done = sym first_done);
}
extern "C" fn first_done(value: usize, control: usize) -> ! {
    assert_eq!((value, control), (0, 0));
    unit::exit(0, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
pub(super) fn run() {
    for entry in [
        first_single as *const () as usize,
        first_double as *const () as usize,
    ] {
        let task = lifecycle::spawn(entry);
        unit::resume(UnitTarget::Task(task)).unwrap();
        assert!(lifecycle::wait_task(task));
        assert_eq!(lifecycle::result(task), (1, 0));
        lifecycle::delete(task);
    }
    for (entry, blocking) in [
        (running as *const () as usize, false),
        (blocked as *const () as usize, true),
    ] {
        STARTED.store(false, Ordering::Release);
        let task = lifecycle::spawn(entry);
        unit::resume(UnitTarget::Task(task)).unwrap();
        let deadline = room::clock() + 5_000_000_000;
        while !STARTED.load(Ordering::Acquire)
            || (blocking
                && lifecycle::inspect(UnitQuery::Target(UnitTarget::Task(task))).state
                    != UnitState::Waiting)
        {
            assert!(room::clock() < deadline);
            room::yield_();
        }
        // Running cancellation can return Busy while its IPI is in flight.
        let _ = unit::stop(UnitTarget::Task(task));
        assert!(lifecycle::wait_task(task));
        let info = lifecycle::inspect(UnitQuery::Target(UnitTarget::Task(task)));
        assert_eq!(info.exit.unwrap().0, ExitCause::Slay);
        lifecycle::delete(task);
    }
    for entry in [
        invalid as *const () as usize,
        active_invalid as *const () as usize,
    ] {
        let task = lifecycle::spawn(entry);
        unit::resume(UnitTarget::Task(task)).unwrap();
        assert!(lifecycle::wait_task(task));
        let info = lifecycle::inspect(UnitQuery::Target(UnitTarget::Task(task)));
        assert_eq!(info.exit, Some((ExitCause::Fault, EXIT_FAULT as u64)));
        lifecycle::delete(task);
    }
    for _ in 0..16 {
        let task = lifecycle::spawn(fresh as *const () as usize);
        unit::resume(UnitTarget::Task(task)).unwrap();
        assert!(lifecycle::wait_task(task));
        assert_eq!(lifecycle::result(task), (1, 0));
        lifecycle::delete(task);
    }
    Console
        .write_str("PASS context faults=2 reuse=16 stop=2\n")
        .unwrap();
}
