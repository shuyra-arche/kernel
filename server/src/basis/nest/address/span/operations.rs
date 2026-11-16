//! Logical position transitions owned by the Span.
use alloc::sync::Arc;

use super::{Access, Span};
use crate::basis::cell::page::Change;
use crate::basis::nest::memo::publication::Publication;
use crate::basis::nest::{
    Nest, NestError, authority,
    memo::{self, source::Source},
    sign,
};
use crate::basis::sign::WakeKey;
use crate::metal::mmu::internal::identity;
use kernel_api::address::{PageOffset, VirtAddr};

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

impl Span {
    pub(crate) fn publish(
        self: &Arc<Self>,
        nest: &Arc<Nest>,
        index: usize,
        expected: u64,
        change: Arc<Change>,
        publisher: memo::Publisher,
        mut maintenance: Option<Arc<crate::basis::pend::MaintenanceUse>>,
        accepted: &mut impl FnMut(u64),
    ) -> Result<(), crate::basis::nest::memo::AccessError> {
        use crate::basis::nest::memo::AccessError;
        let _operation = self.operation().ok_or(AccessError::Busy)?;
        nest.ensure_span(self)?;
        if !nest.is_external() {
            nest.finish_invalidation()?;
        }
        self.finish_memo(index);
        let (_, access) = self.access(index)?;
        let principal = publisher.observe();
        let mut publisher = Some(publisher);
        let commit = crate::basis::commit();
        if !nest.is_open() || !self.is_open() {
            return Err(AccessError::Denied);
        }
        let actor = principal.check(nest, self, index, access)?;
        let (page, source, binding) = {
            let state = self.state.lock();
            let memo = state.memos.get(index).ok_or(AccessError::SourceEmpty)?;
            if memo.generation() != expected || memo.busy() {
                return Err(AccessError::Busy);
            }
            memo.can_advance(expected)?;
            memo.association().ok_or(AccessError::SourceEmpty)?
        };
        let page = match page {
            Some(page) => page,
            None => source
                .range()?
                .page(PageOffset::ZERO)?
                .ok_or(AccessError::SourceEmpty)?,
        };
        if !Arc::ptr_eq(&page, change.page()) || page.level() != self.level() {
            return Err(AccessError::Busy);
        }
        let _provenance = source.hold_provenance()?;
        source.check(access)?;
        if let Some(origin) = source.provenance() {
            origin
                .remember(&binding)
                .map_err(|_| AccessError::OutOfMemory)?;
        }
        if let Some(usage) = &maintenance {
            usage.check(&source, access)?;
        }
        let maintenance_owner = maintenance
            .as_ref()
            .and_then(|usage| usage.source_owner(&source));
        let range = source.same_page(&page)?;
        if nest.is_external() {
            if index != 0 {
                return Err(AccessError::NoRegion);
            }
            // The source loan may enumerate Nest bindings. No Nest/source lock is held.
            let mut borrow = Some(source.borrow(
                &actor,
                &page,
                access.contains(Access::WRITE),
                &change,
                maintenance_owner.as_deref(),
            )?);
            if !nest.is_open() || !self.is_open() {
                return Err(AccessError::Denied);
            }
            principal.check(nest, self, index, access)?;
            nest.prepare_external(
                self,
                expected,
                &page,
                change,
                access.contains(Access::WRITE),
                &mut publisher,
                &mut borrow,
                &mut maintenance,
                source.provenance(),
            )
            .map_err(memo::publication_failure)?;
            accepted(expected.checked_add(1).expect("prepared generation"));
            // Responsibility is in Memo, and its loan/route are marked published
            // before the first backend write, including a partially failed submit.
            let result = nest
                .submit_external(self)
                .map_err(memo::publication_failure);
            drop(commit);
            sign::signal(WakeKey::ExternalSpan {
                id: self.identity(),
            });
            return result;
        }
        let flight = source.begin_cpu(self.identity(), access, maintenance_owner.as_deref())?;
        let lease = range.reside_during(&change)?;
        source.same_page(&page)?;
        principal.check(nest, self, index, access)?;
        // A prepared executable membership excludes writers before FENCE.I,
        // and stays alive until the real PTE publication owns that obligation.
        let instructions = if access.contains(Access::EXECUTE) {
            Some(
                Publication::prepare_cpu(
                    &page,
                    false,
                    &change,
                    maintenance.as_ref().map(|usage| usage.permit()),
                    crate::metal::generic::memory::Endpoint::Cpu {
                        nest: core::num::NonZeroU64::new(nest.id.get() as u64).unwrap(),
                        leases: core::num::NonZeroU64::new(nest.id.get() as u64).unwrap(),
                    },
                    crate::metal::generic::memory::Access::new((access.bits() << 1) as u8)
                        .ok_or(AccessError::Denied)?,
                )
                .map_err(NestError::from)?,
            )
        } else {
            None
        };
        let commit = if instructions.is_some() {
            drop(commit);
            identity::sync_instructions().map_err(AccessError::from)?;
            let commit = crate::basis::commit();
            if !nest.is_open() || !self.is_open() {
                return Err(AccessError::Denied);
            }
            principal.check(nest, self, index, access)?;
            source.check(access)?;
            source.same_page(&page)?;
            if let Some(usage) = &maintenance {
                usage.check(&source, access)?;
            } else {
                source.admit_provenance(None)?;
                source
                    .source()
                    .check_publish(source.window().0, source.window().1)
                    .map_err(|_| AccessError::Denied)?;
            }
            commit
        } else {
            commit
        };
        let prepared = {
            let mut state = self.translation.lock();
            if !state.retirement.invalidation().is_clean() {
                return Err(AccessError::Busy);
            }
            if !state.is_open() || !self.is_open() {
                return Err(AccessError::Denied);
            }
            {
                let mut inner = self.state.lock();
                let memo = inner.memos.existing_mut(index).ok_or(AccessError::Denied)?;
                if !memo.matches(&binding) {
                    return Err(AccessError::Busy);
                }
                self.prepare_revision()?;
                memo.begin_publish(expected, change.clone(), access.bits() << 1)?;
                self.changed();
                memo.retain(
                    publisher.take().expect("prepared publishing route"),
                    None,
                    maintenance.clone(),
                    source.provenance(),
                );
                accepted(memo.generation());
                self.faults
                    .lock()
                    .publishing(index, expected, memo.generation());
            }
            state
                .publish_page(
                    nest.geometry(),
                    crate::metal::generic::memory::Endpoint::Cpu {
                        nest: core::num::NonZeroU64::new(nest.id.get() as u64).unwrap(),
                        leases: core::num::NonZeroU64::new(nest.id.get() as u64).unwrap(),
                    },
                    self,
                    VirtAddr::new(self.position(index)?),
                    &page,
                    &change,
                    maintenance.as_ref().map(|usage| usage.permit()),
                )
                // Claim the edited batch before exposing it to retry observers.
                // The accepted publisher owns its exact completion invocation.
                .and_then(|()| state.retirement.begin(nest.id.get(), nest.asid()))
        };
        drop(commit);
        // Neither COMMIT nor a Nest/source lock crosses a CPU fence.
        let mut result = prepared
            .and_then(|target| nest.invalidate(target))
            .map_err(AccessError::from);
        let valid = {
            let _commit = crate::basis::commit();
            nest.is_open()
                && self.is_open()
                && principal.check(nest, self, index, access).is_ok()
                && source.check(access).is_ok()
                && source.same_page(&page).is_ok()
                && if let Some(usage) = &maintenance {
                    usage.check_retained(&source, access).is_ok()
                } else {
                    source.admit_provenance(None).is_ok()
                        && source
                            .source()
                            .check_publish(source.window().0, source.window().1)
                            .is_ok()
                }
        };
        if !valid {
            let _ = nest.retract_position(self, index, None, change, true);
            result = Err(AccessError::Denied);
        }
        self.finish_memo(index);
        drop(instructions);
        drop(lease);
        drop(flight);
        sign::signal(WakeKey::Span {
            space: self.identity.nest_id(),
            span: self.identity(),
        });
        result
    }
}
