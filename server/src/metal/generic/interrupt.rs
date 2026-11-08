//! External interrupt identity, claims, and route lifecycle.

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
}
