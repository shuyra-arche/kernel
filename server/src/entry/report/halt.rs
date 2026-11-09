use super::scene::Source;
use core::fmt::Write;
use core::panic::PanicInfo;

#[panic_handler]
pub(crate) fn panic_handler(info: &PanicInfo) -> ! {
    start(&Source::RustPanic(info))
}

pub(crate) fn trap(
    frame: &crate::metal::switcher::context::TrapContext,
    cause: usize,
    value: usize,
) -> ! {
    start(&Source::Trap {
        frame,
        cause,
        value,
    })
}

fn start(source: &Source<'_>) -> ! {
    unsafe { core::arch::asm!("csrci sstatus, 2") };
    if !crate::metal::stop::alarm() {
        let _ = writeln!(
            crate::metal::console::Emergency::new(),
            "[kernel] recursive-failure evidence=incomplete"
        );
        crate::metal::stop::failed()
    }
    crate::metal::stop::home(source)
}

#[allow(improper_ctypes_definitions)]
pub(crate) extern "C" fn info(source: &Source<'_>) -> ! {
    let mut sink = crate::metal::console::Emergency::new();
    let booted = crate::entry::boot::facts::dram_edge().is_some();
    let expected = if booted {
        crate::metal::hart::hart_count().saturating_sub(1)
    } else {
        0
    };
    // Bounded observation only. A missing acknowledgement is never reclamation proof.
    for _ in 0..100_000 {
        if crate::metal::stop::quiesced_peers() == expected {
            break;
        }
        core::hint::spin_loop();
    }
    let quiet = crate::metal::stop::quiesced_peers();
    let root_ok =
        unsafe { (crate::metal::image::root_stack_base() as *const usize).read_volatile() }
            == crate::metal::image::ROOT_STACK_CANARY;
    let _ = writeln!(
        sink,
        "[kernel] fatal hart={} peers-quiet={quiet}/{expected} peers-unobserved={} root-canary={root_ok}",
        crate::metal::hart::hart_id(),
        expected.saturating_sub(quiet)
    );
    if let Some(identity) = crate::basis::room::conductor::identity::ident() {
        let _ = writeln!(
            sink,
            "team={} task={}",
            identity.team_id(),
            identity.task_id()
        );
    }
    // Unacknowledged peers may still change memory. Omit the page-table walk in
    // that case rather than racing freed table pages during crash reporting.
    let _ = super::scene::write(source, &mut sink, booted && quiet == expected);
    sink.finish();
    crate::metal::stop::failed()
}
