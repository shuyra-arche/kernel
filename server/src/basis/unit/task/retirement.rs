//! One husk owns this continuation. A successful stage is never replayed; a
//! failed hardware or authority operation keeps the same stage and ownership.
use super::Task;
use crate::basis::nest::NestError;
use alloc::sync::Arc;

pub(super) enum Retirement {
    Active,
    Heirs { after: usize },
    Pends,
    Memory,
    Receipts(crate::basis::unit::team::Clearance),
    Settled,
}
impl Retirement {
    pub(super) const fn new() -> Self {
        Self::Heirs { after: 0 }
    }
}
impl Task {
    /// A bounded, read-only diagnostic for a shutdown that has stopped
    /// making retirement progress. It does not grant new cleanup authority.
    pub(crate) fn retirement_stage(&self) -> &'static str {
        let Some(phase) = self.retirement.try_lock() else {
            return "in-flight";
        };
        match &*phase {
            Retirement::Active => "in-flight",
            Retirement::Heirs { .. } => "heirs",
            Retirement::Pends => "pends",
            Retirement::Memory => "memory",
            Retirement::Receipts(_) => "receipts",
            Retirement::Settled => "settled",
        }
    }

    /// Observe only the next real child while the husk is stalled, after
    /// dropping the retirement guard. Never materialize a child snapshot.
    pub(crate) fn retirement_heir(&self) -> Option<Arc<crate::basis::unit::team::Team>> {
        let after = match &*self.retirement.try_lock()? {
            Retirement::Heirs { after } => *after,
            _ => return None,
        };
        self.heir
            .try_lock()?
            .iter()
            .filter(|team| team.id.get() > after)
            .min_by_key(|team| team.id.get())
            .cloned()
    }

    /// One owner-controlled step. Ok(false) is an immediately runnable next
    /// stage; Err retains an obligation requiring a later attempt.
    pub(crate) fn advance_retirement(self: &Arc<Self>) -> Result<bool, NestError> {
        let phase = {
            let mut stored = self.retirement.try_lock().ok_or(NestError::Busy)?;
            if matches!(*stored, Retirement::Active) {
                return Err(NestError::Busy);
            }
            core::mem::replace(&mut *stored, Retirement::Active)
        };
        let mut progress = Progress { task: self, phase };
        let phase = &mut progress.phase;
        // Stop closes live authority while admission prepares outside COMMIT.
        // Keep the original resources until that lease finishes reading them.
        // Only reaped Tasks reach retirement, so a new live admission cannot
        // begin after this check.
        if self.preparing() {
            return Err(NestError::Busy);
        }
        match phase {
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
                *phase = Retirement::Memory;
            }
            Retirement::Memory => {
                self.ident.retire()?;
                *phase = Retirement::Receipts(self.ident.receipts());
            }
            Retirement::Receipts(cursor) => {
                if cursor.advance(self.ident.id, None) {
                    *phase = Retirement::Settled;
                    return Ok(true);
                }
            }
            Retirement::Settled => return Ok(true),
            Retirement::Active => unreachable!("only the owned continuation advances retirement"),
        }
        Ok(false)
    }
}

// Move the existing continuation out of storage, leaving one busy marker.
// Hardware work retains its exact resources and stage through this owned lease,
// never an IRQ-masking mutex. Every exit restores the latest completed stage.
struct Progress<'a> {
    task: &'a Task,
    phase: Retirement,
}
impl Drop for Progress<'_> {
    fn drop(&mut self) {
        let mut stored = self.task.retirement.lock();
        assert!(matches!(*stored, Retirement::Active));
        *stored = core::mem::replace(&mut self.phase, Retirement::Active);
    }
}
