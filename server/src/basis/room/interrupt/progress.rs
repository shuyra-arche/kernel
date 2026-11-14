//! The controller Hart retains each masked claim until software accepts it.
//! A separate round-robin cursor bounds maintenance without restarting at zero
//! when a busy source asks for another turn.
use crate::metal::generic::interrupt::Irq;
pub(super) const BATCH: usize = 4;
pub(super) struct Claim {
    pub(super) irq: Irq,
    #[cfg(feature = "latency")]
    pub(super) entered: u64,
}
pub(super) struct Progress {
    claims: [Option<Claim>; BATCH],
    next: usize,
    requested: u64,
}
impl Progress {
    pub(super) const fn new() -> Self {
        Self {
            claims: [const { None }; BATCH],
            next: 0,
            requested: 0,
        }
    }
    pub(super) fn vacant(&self) -> bool {
        self.claims.iter().any(Option::is_none)
    }
    pub(super) fn retain(&mut self, claim: Claim) {
        let slot = self
            .claims
            .iter_mut()
            .find(|slot| slot.is_none())
            .expect("claim capacity checked before MMIO");
        *slot = Some(claim);
    }
    pub(super) fn take(&mut self) -> [Option<Claim>; BATCH] {
        core::mem::replace(&mut self.claims, [const { None }; BATCH])
    }
    pub(super) fn request(&mut self, index: usize) {
        assert!(index < 64, "registered Source slot");
        self.requested |= 1u64 << index;
    }
    pub(super) fn refresh(&mut self, count: usize) {
        assert!(count <= 64);
        self.requested |= if count == 64 {
            u64::MAX
        } else {
            (1u64 << count) - 1
        };
    }
    pub(super) fn next(&mut self, count: usize) -> Option<usize> {
        if count == 0 {
            self.requested = 0;
            return None;
        }
        if self.requested == 0 {
            return None;
        }
        let offset = self
            .requested
            .rotate_right(self.next as u32)
            .trailing_zeros() as usize;
        let at = (self.next + offset) % 64;
        self.requested &= !(1u64 << at);
        self.next = (at + 1) % 64;
        Some(at)
    }
    pub(super) fn pending(&self) -> bool {
        self.requested != 0 || self.claims.iter().any(Option::is_some)
    }
}
