//! Logical position transitions owned by the Span.
use alloc::sync::Arc;

use super::Span;
use crate::basis::nest::{
    Nest, NestError, authority,
    memo::{self, source::Source},
    sign,
};
use crate::basis::sign::WakeKey;

impl Span {
    /// Insert a logical association into one position. The caller holds COMMIT
    /// and the actor's live authority.
    pub(crate) fn insert(
        self: &Arc<Self>,
        nest: &Arc<Nest>,
        index: usize,
        expected: u64,
        source: Source,
    ) -> Result<(), NestError> {
        if self.source().is_some() {
            return Err(NestError::Pinned);
        }
        let _operation = self.operation().ok_or(NestError::Busy)?;
        let _commit = crate::basis::commit();
        nest.ensure_span(self)?;
        self.access(index)?;
        if !nest.is_open() || !self.is_open() {
            return Err(NestError::NoRegion);
        }
        let range = source.range().map_err(memo::source_error)?;
        if range.level() != self.level() {
            return Err(NestError::NotAligned);
        }
        let (offset, count) = source.window();
        source
            .source()
            .check_move(offset, count, source.hold().authority().map(Arc::as_ref))
            .map_err(memo::namespace_error)?;
        source
            .source()
            .check_publish(offset, count)
            .map_err(memo::namespace_error)?;
        let binding = authority::RangeBinding::new_position(self.identity(), nest, index)
            .map_err(memo::namespace_error)?;
        source
            .source()
            .reserve_binding()
            .map_err(memo::namespace_error)?;

        let translation = self.translation.lock();
        if !translation.retirement.invalidation().is_clean() {
            return Err(NestError::Busy);
        }
        {
            let mut inner = self.state.lock();
            let memo = inner.memos.get_mut(index)?;
            if !memo.accepts_logical() {
                return Err(NestError::Pinned);
            }
            memo.can_associate(expected)?;
            self.prepare_revision()?;
        }
        if !nest.is_external() {
            crate::basis::nest::index::source::register(&range, self, Some(&binding))?;
        }
        source.range().map_err(memo::source_error)?;
        let authorization = source.source().clone();
        let replaced = {
            if !translation.is_open() || !self.is_open() {
                return Err(NestError::NoRegion);
            }
            let mut inner = self.state.lock();
            inner
                .memos
                .existing_mut(index)
                .expect("prepared Memo")
                .associate(expected, None, source, binding.clone())
        };
        self.changed();
        authorization.register_binding(binding);
        replaced.release();
        self.faults
            .lock()
            .invalidate(Some(index), kernel_api::abi::memory::Failure::Stale);
        sign::signal(WakeKey::Span {
            space: self.identity.nest_id(),
            span: self.identity(),
        });
        Ok(())
    }

    /// Remove the logical association from one position. This never starts
    /// unrelated hardware work under its atomic commit.
    pub(crate) fn remove(
        self: &Arc<Self>,
        nest: &Arc<Nest>,
        index: usize,
        expected: u64,
    ) -> Result<(), NestError> {
        let _operation = self.operation().ok_or(NestError::Busy)?;
        nest.ensure_span(self)?;
        self.position(index)?;
        if self.source().is_some() {
            return Err(NestError::Pinned);
        }
        let _commit = crate::basis::commit();
        let removed = {
            let translation = self.translation.lock();
            if !translation.retirement.invalidation().is_clean() {
                return Err(NestError::Busy);
            }
            let mut inner = self.state.lock();
            let memo = inner.memos.get_mut(index)?;
            if !memo.accepts_logical() {
                return Err(NestError::Pinned);
            }
            self.prepare_revision()?;
            let removed = memo.remove(expected)?;
            self.changed();
            removed
        };
        removed.release();
        self.faults
            .lock()
            .invalidate(Some(index), kernel_api::abi::memory::Failure::Stale);
        sign::signal(WakeKey::Span {
            space: self.identity.nest_id(),
            span: self.identity(),
        });
        Ok(())
    }
}

impl Span {
    /// The exact local Memo state for one page, without owning a Nest lookup.
    pub(crate) fn memo_page(
        &self,
        index: usize,
    ) -> Result<Option<Arc<crate::basis::cell::page::Page>>, NestError> {
        self.position(index)?;
        let state = self.state.lock();
        Ok(state.memos.get(index).and_then(|memo| memo.page()))
    }

    pub(in crate::basis::nest) fn memo_published_by(
        &self,
        index: usize,
        authority: &Arc<crate::basis::pend::Pend>,
    ) -> bool {
        self.state
            .lock()
            .memos
            .get(index)
            .is_some_and(|memo| memo.published_by(authority))
    }

    /// Settle the local Memo only when no translation batch is outstanding.
    pub(crate) fn finish_memo(&self, index: usize) {
        let settled = {
            let translation = self.translation.lock();
            if !translation.retirement.invalidation().is_clean() {
                return;
            }
            let mut inner = self.state.lock();
            inner.memos.existing_mut(index).map(|memo| {
                let before = memo.inspect(kernel_api::address::VirtAddr::new(0));
                let settled = memo.finish();
                if memo.inspect(kernel_api::address::VirtAddr::new(0)) != before {
                    self.changed();
                }
                settled
            })
        };
        if let Some(settled) = settled {
            settled.release();
        }
    }
}
