//! Device custody is separate from DMA business. Configuration and BAR effects
//! stay behind one Kernel object; caller identity is an admission token, never
//! a physical address supplied by a Unit.
pub(crate) mod pci;
mod phase;
pub(crate) use phase::Phase;
