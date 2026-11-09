//! External interrupt identity, claims, and route lifecycle.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Irq {
    pub(crate) controller: usize,
    pub(crate) line: u32,
}

/// Serialized controller mechanism; no Task, Sign or ownership policy here.
pub(crate) trait Delivery: Send + Sync {
    /// Claim and mask; the caller retains the outstanding controller claim.
    fn claim_masked(&self) -> Option<Irq>;
    fn complete(&self, irq: Irq);
    fn mask(&self, irq: Irq);
    fn unmask(&self, irq: Irq);
    fn prioritize(&self, irq: Irq);
}
