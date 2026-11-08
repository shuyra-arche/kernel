use riscv::register::{sie, sip};

pub(crate) fn enable_external() {
    unsafe { sie::set_sext() };
}

pub(crate) fn disable_external() {
    unsafe { sie::clear_sext() };
}

pub(crate) fn external_pending() -> bool {
    sip::read().sext()
}
