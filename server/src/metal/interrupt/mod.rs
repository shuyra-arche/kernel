pub(crate) mod plic;
mod riscv;

pub(crate) use riscv::{disable_external, enable_external, external_pending, ipi};

use alloc::boxed::Box;

use crate::metal::sync::OnceLock;

use crate::metal::generic::interrupt::External;

static EXTERNAL: OnceLock<Box<dyn External>> = OnceLock::new();

pub(crate) fn install_external(route: Box<dyn External>) -> Result<(), ()> {
    EXTERNAL.set(route).map_err(|_| ())
}

pub(crate) fn external() -> Option<&'static dyn External> {
    EXTERNAL.get().map(Box::as_ref)
}
