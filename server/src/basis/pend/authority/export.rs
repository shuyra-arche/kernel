//! A retained logical derivation. Its fixed selection is descriptive; every use
//! still consults the original live Pend. Memo retains it through retirement.
use super::{Arc, Distributable, Modifiable, Operations, Ordering, Pend, PendFail, PendScope};

pub(crate) struct Export {
    authority: Arc<Pend>,
    operations: Operations,
    scope: PendScope,
}

impl Export {
    /// Caller holds COMMIT through installation or cancellation. No copied
    /// Limits authorize a derivation, and no Page/publication is created here.
    pub(crate) fn new(
        authority: &Arc<Pend>,
        operations: Operations,
        scope: PendScope,
    ) -> Result<Self, PendFail> {
        if !authority.alive() {
            return Err(PendFail::Dead);
        }
        let state = authority.state.lock();
        if !state.selects(operations, scope) || state.distributable != Distributable::Yes {
            return Err(PendFail::Denied);
        }
        authority
            .exports
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            .map_err(|_| PendFail::Busy)?;
        Ok(Self {
            authority: authority.clone(),
            operations,
            scope,
        })
    }

    pub(crate) fn authority(&self) -> &Arc<Pend> {
        &self.authority
    }

    pub(crate) fn permits(&self, access: Modifiable) -> bool {
        if !self.authority.alive() {
            return false;
        }
        let state = self.authority.state.lock();
        state.selects(self.operations, self.scope)
            && state.distributable == Distributable::Yes
            && state.access.allows(access)
    }
}

impl Drop for Export {
    fn drop(&mut self) {
        // This releases a logical dependency, never a hardware receipt. A busy
        // Memo cannot remove its Source, so its exports outlive exact retirement.
        assert!(self.authority.exports.fetch_sub(1, Ordering::AcqRel) > 0);
    }
}
