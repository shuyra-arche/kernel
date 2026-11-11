//! A Pend target refers to the Nest's authoritative Memo record. It owns no Page,
//! association or parallel mapping table; live limits remain in the Pend ledger.
use crate::basis::{
    nest::{Nest, Span},
    pend::{self, PendEntry},
    unit::task::Task,
};
use alloc::sync::Arc;
use kernel_api::{
    abi::call::{cell::CellFail, pend::PendFail},
    address::{PageCount, PageOffset, VirtAddr},
    model::{
        cell::{CellState, MappingId, MemoId},
        identity::{Mark, PendId, TaskId},
        modifiable::Modifiable,
        pend::{MemoRights, Operations},
    },
};

pub(crate) struct Memo {
    pub(crate) id: MemoId,
    pub(crate) owner: TaskId,
    pub(crate) nest: Arc<Nest>,
    pub(crate) mapping: MappingId,
    pub(crate) count: PageCount,
}
impl Memo {
    pub(crate) fn register(
        actor: &Arc<Task>,
        nest: Arc<Nest>,
        mapping: MappingId,
        count: PageCount,
    ) -> Result<PendId, CellFail> {
        // Occupancy allocates a global, never-reused relationship identity.
        let id = MemoId::new(mapping.get()).ok_or(CellFail::Denied)?;
        let memo = Arc::try_new(Self {
            id,
            owner: actor.ident.id,
            nest,
            mapping,
            count,
        })
        .map_err(|_| CellFail::OutOfMemory)?;
        let entry = PendEntry::try_new_window(
            pend::Target::Memo(memo),
            Mark::NONE,
            Modifiable::Yes,
            None,
            Some((PageOffset::ZERO, count)),
        )
        .map_err(failure)?;
        let token = entry.token();
        pend::insert(actor, entry).map_err(failure)?;
        Ok(token)
    }
    pub(crate) fn alive(&self) -> bool {
        !self.nest.is_closed()
    }
    pub(crate) fn span(&self) -> Result<Arc<Span>, CellFail> {
        self.nest
            .mappings
            .locate(self.mapping.get())
            .and_then(|e| e.mapping)
            .ok_or(CellFail::Denied)
    }
    /// Revoke/Return must settle the selected CPU window before dropping its
    /// authority. Nest retains failed invalidation; retries use the same record.
    pub(crate) fn withdraw(&self, entry: &PendEntry) -> Result<(), PendFail> {
        if entry.authority().external_pending() {
            return Err(PendFail::Busy);
        }
        if self.nest.is_external() {
            return super::external::withdraw_memo(self, entry);
        }
        let Some(record) = self.nest.mappings.locate(self.mapping.get()) else {
            return Ok(());
        };
        let span = record.mapping().ok_or(PendFail::Busy)?;
        let _operation = span.operation().ok_or(PendFail::Busy)?;
        self.nest.retry_invalidation().map_err(|_| PendFail::Busy)?;
        if entry.sire().is_none() && entry.authority().retiring() {
            self.nest
                .dispose(span.clone())
                .map_err(|_| PendFail::Busy)?;
            self.nest.retire_mapping(self.mapping.get());
            return if self.nest.settled(self.mapping.get()) {
                Ok(())
            } else {
                Err(PendFail::Busy)
            };
        }
        let (base, count) = entry
            .authority()
            .window()
            .unwrap_or((PageOffset::ZERO, self.count));
        for page in (base.get()..base.get() + count.get()).step_by(span.source().grain()) {
            self.nest.finish_memo(span, page);
            if !self.nest.memo_published_by(span, page, entry.authority()) {
                continue;
            }
            let info = span.inspect_memo(page).map_err(|_| PendFail::Busy)?;
            if matches!(info.state, CellState::Published | CellState::Publishing) {
                let data = self
                    .nest
                    .memo_page(span, page)
                    .map_err(|_| PendFail::Busy)?
                    .ok_or(PendFail::Busy)?;
                self.nest
                    .unpublish_page(
                        span,
                        VirtAddr::new(span.initial_start().get() + page * 4096),
                        &data,
                    )
                    .map_err(|_| PendFail::Busy)?;
                self.nest.finish_memo(span, page);
            } else if !matches!(info.state, CellState::Ready | CellState::Vacant) {
                return Err(PendFail::Busy);
            }
        }
        Ok(())
    }
}
fn failure(error: PendFail) -> CellFail {
    match error {
        PendFail::OutOfMemory => CellFail::OutOfMemory,
        PendFail::Busy => CellFail::Busy,
        _ => CellFail::Denied,
    }
}
/// Keeps Narrow/cleanup from completing across a hardware operation. Logical
/// revocation may still close admission; callers revalidate before success.
pub(crate) struct Lease(pub(crate) Arc<PendEntry>);
impl Lease {
    pub(crate) fn acquire(
        actor: &Task,
        token: PendId,
        rights: MemoRights,
    ) -> Result<Self, CellFail> {
        let _commit = crate::basis::commit();
        if !pend::live(actor) {
            return Err(CellFail::Denied);
        }
        let entry = pend::locate(actor, token)
            .filter(|e| e.permits(Operations::Memo(rights)))
            .ok_or(CellFail::Denied)?;
        if !matches!(entry.target(), pend::Target::Memo(_)) {
            return Err(CellFail::Denied);
        }
        entry.authority().retain_route().map_err(failure)?;
        Ok(Self(entry))
    }
    pub(crate) fn memo(&self) -> &Arc<Memo> {
        let pend::Target::Memo(memo) = self.0.target() else {
            unreachable!()
        };
        memo
    }
    pub(crate) fn page(&self, relative: usize) -> Result<usize, CellFail> {
        let (base, count) = self.0.authority().window().ok_or(CellFail::Denied)?;
        if relative >= count.get() {
            return Err(CellFail::NoRegion);
        }
        base.get().checked_add(relative).ok_or(CellFail::NotAligned)
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        unsafe { self.0.authority().release_route() };
    }
}
