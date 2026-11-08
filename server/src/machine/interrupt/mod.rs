pub(crate) mod ipi;
pub(crate) mod plic;

use riscv::register::{sie, sip};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Irq {
    pub(crate) controller: usize,
    pub(crate) line: u32,
}

/// An interrupt claimed from a particular controller context. Finish consumes
/// the claim and leaves the line masked until the deferred work can rearm it.
pub(crate) trait Claim {
    fn finish(self) -> Irq;
}

pub(crate) trait Controller {
    fn claim(&self) -> Option<impl Claim + '_>;
    fn unmask(&self, irq: Irq);
}

pub(crate) fn enable_external() {
    unsafe { sie::set_sext() };
}

pub(crate) fn disable_external() {
    unsafe { sie::clear_sext() };
}

pub(crate) fn external_pending() -> bool {
    sip::read().sext()
}
