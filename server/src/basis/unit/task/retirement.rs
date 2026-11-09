//! One husk owns this continuation. A successful stage is never replayed; a
//! failed hardware or authority operation keeps the same stage and ownership.
use super::Task;
use crate::basis::nest::NestError;
use alloc::sync::Arc;

pub(super) enum Retirement {
    Heirs { after: usize },
    Pends,
    Controls,
    Memory,
    Settled,
}
impl Retirement {
    pub(super) const fn new() -> Self {
        Self::Heirs { after: 0 }
    }
}
impl Task {
    /// Advance one child or one local stage, without recursive Task retirement.
    /// Busy retains the husk. The phase guard is tried, never waited on.
    pub(crate) fn retire(self: &Arc<Self>) -> Result<(), NestError> {
        let mut phase = self.retirement.try_lock().ok_or(NestError::Busy)?;
        match &mut *phase {
            Retirement::Heirs { after } => {
                // Team identities never repeat. Concurrent Oust may remove an
                // earlier child, so a vector index cannot be the continuation.
                let child = self
                    .heir
                    .lock()
                    .iter()
                    .filter(|team| team.id.get() > *after)
                    .min_by_key(|team| team.id.get())
                    .cloned();
                if let Some(child) = child {
                    child.retire()?;
                    *after = child.id.get();
                } else {
                    *phase = Retirement::Pends;
                }
            }
            Retirement::Pends => {
                crate::basis::pend::retire(self).map_err(|_| NestError::Busy)?;
                *phase = Retirement::Controls;
            }
            Retirement::Controls => {
                self.controls.retire(self.ident.id)?;
                *phase = Retirement::Memory;
            }
            Retirement::Memory => {
                self.ident.retire()?;
                *phase = Retirement::Settled;
                return Ok(());
            }
            Retirement::Settled => return Ok(()),
        }
        Err(NestError::Busy)
    }
}
