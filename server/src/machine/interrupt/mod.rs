pub(crate) mod ipi;
pub(crate) mod plic;

use alloc::boxed::Box;
use riscv::register::{sie, sip};

use crate::machine::sync::OnceLock;

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

/// An optional Kernel-owned external route selected at boot. Work may run on
/// any Hart, while the implementation limits register access to its owner.
pub(crate) trait External: Send + Sync {
    fn handle(&self) -> bool;
    fn rearm(&self);
    fn overlaps(&self, base: usize, end: usize) -> bool;
}

static EXTERNAL: OnceLock<Box<dyn External>> = OnceLock::new();

pub(crate) fn install_external(route: Box<dyn External>) -> Result<(), ()> {
    EXTERNAL.set(route).map_err(|_| ())
}

pub(crate) fn external() -> Option<&'static dyn External> {
    EXTERNAL.get().map(Box::as_ref)
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
