//! One address position, identified by (MappingId, page offset).
//! Missing sparse entries are Unmapped; publication survives until its fence.
use crate::basis::cell::page::{Change, Page, RetainError, Retention};
use alloc::sync::Arc;
use core::ops::Deref;

pub(crate) enum Memo {
    Unmapped,
    Publishing(Retention),
    Mapped(Retention),
    Withdrawing(Retention),
}
impl Memo {
    pub(crate) fn prepare(
        page: &Arc<Page>,
        writable: bool,
        change: Option<&Change>,
    ) -> Result<Self, RetainError> {
        let retention = match change {
            Some(change) if Arc::ptr_eq(change.page(), page) => change.hardware(writable)?,
            Some(_) => return Err(RetainError::Busy),
            None => page.prepare_hardware(writable)?,
        };
        Ok(Self::Publishing(retention))
    }
    /// Before the first hardware write; a dropped published obligation is quarantined.
    pub(crate) fn publish(&mut self) {
        let Self::Publishing(retention) = self else {
            panic!("publish requires Publishing")
        };
        retention.publish();
    }
    /// Hardware publication and synchronization have both completed.
    pub(crate) fn complete(&mut self) {
        let old = core::mem::replace(self, Self::Unmapped);
        *self = match old {
            Self::Publishing(retention) => Self::Mapped(retention),
            other => other,
        };
    }
    pub(crate) fn retire(self) -> Self {
        match self {
            Self::Publishing(retention) | Self::Mapped(retention) => {
                Self::Withdrawing(retention.retire())
            }
            _ => panic!("withdraw requires a published Memo"),
        }
    }
    /// # Safety
    /// All hardware access through this Memo has been withdrawn and synchronized.
    pub(crate) unsafe fn settle(self) {
        let Self::Withdrawing(retention) = self else {
            panic!("settle requires Withdrawing")
        };
        unsafe { retention.settle() };
    }
}
impl Deref for Memo {
    type Target = Retention;
    fn deref(&self) -> &Retention {
        match self {
            Self::Publishing(r) | Self::Mapped(r) | Self::Withdrawing(r) => r,
            Self::Unmapped => panic!("unmapped position has no Page"),
        }
    }
}
