use ::core::panic::PanicInfo;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::entry::report::record::Report;
use crate::metal::hart;
use crate::metal::stop::alarm;

#[panic_handler]
pub(crate) fn panic_handler(info: &PanicInfo) -> ! {
    crash_scene(info)
}

fn crash_scene(info: &PanicInfo) -> ! {
    if !alarm() {
        crate::putln!(
            "info: {} sepc={:#x} stval={:#x}",
            info.message(),
            crate::metal::stop::trap_scene().0,
            crate::metal::stop::trap_scene().1,
        );
        crate::metal::stop::failed()
    }
    crate::metal::stop::home(info)
}

#[allow(improper_ctypes_definitions)]
pub(crate) extern "C" fn info(info: &PanicInfo) -> ! {
    let root_ok =
        unsafe { (crate::metal::image::root_stack_base() as *const usize).read_volatile() }
            == crate::metal::image::ROOT_STACK_CANARY;

    crate::metal::allocator::portal::switch(crate::metal::allocator::portal::Backend::Spare);

    let mut report = Report::default();
    {
        let mut head = String::from("[panic]");
        if let Some(loc) = info.location() {
            head.push_str(&format!(
                " at {}:{}:{}",
                loc.file(),
                loc.line(),
                loc.column()
            ));
        }
        let mut rows: Vec<Vec<Option<String>>> = Vec::new();
        if let Some(i) = crate::basis::room::conductor::identity::ident() {
            rows.push(vec![Some(format!(
                "team #{} / task #{} @ hart {}",
                i.team_id(),
                i.task_id(),
                hart::hart_id()
            ))]);
        }
        rows.push(vec![Some(format!("{}", info.message()))]);
        rows.push(vec![Some(format!(
            "root stack @ {} : {}",
            if root_ok { "ok" } else { "CORRUPTED" },
            hart::hart_id()
        ))]);
        report.paragraph("panic", Some(head)).items.extend(rows);
    }

    crate::entry::report::scene::dump(&mut report);

    let sealed = report.seal();
    crate::putln!();
    let mut sink = crate::metal::console::Sink;
    crate::entry::report::render::render(sealed, &mut sink, 2);
    #[cfg(feature = "semihosting")]
    crate::entry::report::export::export(sealed);

    crate::metal::stop::failed()
}
