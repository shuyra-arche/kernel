//! Logical position transitions owned by the Span.
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::{Access, DrainStep, Span};
use crate::basis::cell::page::{Change, Freeze, Page};
use crate::basis::nest::memo::publication::Publication;
use crate::basis::nest::{
    Nest, NestError, NestRange, authority,
    memo::{self, source::Source},
    sign,
};
use crate::basis::sign::WakeKey;
use crate::metal::generic::mmu::internal::PageTable;
use crate::metal::mmu::internal::identity;
use kernel_api::address::{PageCount, PageOffset, VirtAddr};

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
            let _ = self.retract_forced(nest, index, change);
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

impl Span {
    /// Retract one position under the caller's live authority.
    pub(crate) fn retract(
        self: &Arc<Self>,
        nest: &Nest,
        index: usize,
        expected: u64,
        change: Arc<Change>,
        authorize: &mut impl FnMut() -> Result<(), memo::AccessError>,
        accepted: &mut impl FnMut(u64),
    ) -> Result<(), memo::AccessError> {
        let _operation = self.operation().ok_or(memo::AccessError::Busy)?;
        nest.ensure_span(self)?;
        self.position(index)?;
        let result = self.retract_position(
            nest,
            index,
            Some(expected),
            change,
            false,
            false,
            authorize,
            accepted,
        );
        sign::signal(WakeKey::Span {
            space: self.identity.nest_id(),
            span: self.identity(),
        });
        if nest.is_external() {
            sign::signal(WakeKey::ExternalSpan {
                id: self.identity(),
            });
        }
        result
    }

    /// Forced cleanup uses the same local transition without user authority.
    pub(in crate::basis::nest) fn retract_forced(
        self: &Arc<Self>,
        nest: &Nest,
        index: usize,
        change: Arc<Change>,
    ) -> Result<(), memo::AccessError> {
        self.retract_position(
            nest,
            index,
            None,
            change,
            true,
            false,
            &mut || Ok(()),
            &mut |_| {},
        )
    }

    pub(in crate::basis::nest) fn retract_polling(
        self: &Arc<Self>,
        nest: &Nest,
        index: usize,
        change: Arc<Change>,
        polling: bool,
    ) -> Result<(), memo::AccessError> {
        self.retract_position(
            nest,
            index,
            None,
            change,
            true,
            polling,
            &mut || Ok(()),
            &mut |_| {},
        )
    }

    /// The caller owns span.operation. Forced cleanup never wraps generation MAX.
    fn retract_position(
        self: &Arc<Self>,
        nest: &Nest,
        index: usize,
        expected: Option<u64>,
        change: Arc<Change>,
        forced: bool,
        polling: bool,
        authorize: &mut impl FnMut() -> Result<(), memo::AccessError>,
        accepted: &mut impl FnMut(u64),
    ) -> Result<(), memo::AccessError> {
        if !nest.is_external() {
            nest.finish_invalidation_with(polling)?;
        }
        self.finish_memo(index);
        let page = {
            let _commit = crate::basis::commit();
            authorize()?;
            let mut state = self.state.lock();
            let Some(memo) = state.memos.existing_mut(index) else {
                return if forced {
                    Ok(())
                } else {
                    Err(memo::AccessError::SourceEmpty)
                };
            };
            if expected.is_some_and(|expected| memo.generation() != expected) {
                return Err(memo::AccessError::Busy);
            }
            let page = memo.page().ok_or(memo::AccessError::SourceEmpty)?;
            if !Arc::ptr_eq(&page, change.page()) {
                return Err(memo::AccessError::Busy);
            }
            if memo.publication().is_none() {
                return if !memo.busy() && (forced || memo.frozen()) {
                    Ok(())
                } else {
                    Err(memo::AccessError::Busy)
                };
            }
            if !memo.retracting() {
                if !forced {
                    self.prepare_revision()?;
                }
                memo.begin_retract(memo.generation(), change, forced)?;
                self.changed();
            }
            accepted(memo.generation());
            page
        };
        if nest.is_external() {
            nest.progress_external(self)
                .map_err(memo::publication_failure)?;
        } else {
            let result = self.unpublish_page_with(
                nest,
                VirtAddr::new(self.position(index)?),
                &page,
                polling,
            );
            self.finish_memo(index);
            result?;
        }
        let state = self.state.lock();
        if state.memos.get(index).is_some_and(memo::Memo::busy) {
            Err(memo::AccessError::Busy)
        } else {
            Ok(())
        }
    }
}

impl Span {
    pub(crate) fn protect(
        self: &Arc<Self>,
        nest: &Arc<Nest>,
        index: usize,
        expected: u64,
        access: Access,
        authority: &Arc<crate::basis::pend::Pend>,
        actor: Option<&Arc<crate::basis::unit::task::Task>>,
        accepted: &mut impl FnMut(u64),
    ) -> Result<(), crate::basis::nest::memo::AccessError> {
        use crate::basis::nest::memo::AccessError;
        use kernel_api::model::modifiable::Modifiable;
        use kernel_api::model::pend::{NestRights, NestScope, Operations, PendScope};
        let _operation = self.operation().ok_or(AccessError::Busy)?;
        nest.ensure_span(self)?;
        if nest.is_external() || !access.is_leaf_set() {
            return Err(AccessError::Denied);
        }
        nest.finish_invalidation()?;
        self.finish_memo(index);
        let (ceiling, _) = self.access(index)?;
        let count = PageCount::new(self.grain()).ok_or(AccessError::NotAligned)?;
        let admitted = || {
            nest.is_open()
                && self.is_open()
                && authority.permits(Operations::Nest(NestRights::PROTECT))
                && matches!(authority.scope(), PendScope::Nest(NestScope::Span { id, .. })
                    if id.get() == self.identity())
                && authority.contains(PageOffset::new(index), count)
                && (!access.contains(Access::WRITE)
                    || authority.modifiable() == Some(Modifiable::Yes))
        };
        let commit = crate::basis::commit();
        if !admitted() {
            return Err(AccessError::Denied);
        }
        let ((page, source, binding), retained, maintenance) = {
            let state = self.state.lock();
            let memo = state.memos.get(index).ok_or(AccessError::SourceEmpty)?;
            if memo.generation() != expected || memo.in_flight() {
                return Err(AccessError::Busy);
            }
            memo.can_advance(expected)?;
            (
                {
                    let (_, source, binding) =
                        memo.association().ok_or(AccessError::SourceEmpty)?;
                    (memo.page(), source, binding)
                },
                memo.publication().map(|publication| publication.writable()),
                memo.maintenance().cloned(),
            )
        };
        if !ceiling.contains(access) || !source.ceiling().contains(access) {
            return Err(AccessError::WidenDenied);
        }
        let _provenance = source.hold_provenance()?;
        let range = source.check(access)?;
        if let Some(usage) = &maintenance {
            if !actor.is_some_and(|actor| usage.owned_by(actor)) {
                return Err(AccessError::Denied);
            }
            usage.check(&source, access)?;
        }
        let maintenance_owner = maintenance
            .as_ref()
            .and_then(|usage| usage.source_owner(&source));
        if page.is_none() {
            self.prepare_revision()?;
            let selection = self.range(PageOffset::new(index), count)?;
            let mut inner = self.state.lock();
            {
                let memo = inner
                    .memos
                    .existing_mut(index)
                    .ok_or(AccessError::SourceEmpty)?;
                if !memo.logical() || memo.busy() || !memo.matches(&binding) {
                    return Err(AccessError::Busy);
                }
                memo.can_advance(expected)?;
            }
            let transition = inner.protection_transition(
                self.initial_start(),
                selection.window(),
                super::Protection::SetAccess(access),
                |_| false,
            )?;
            inner.active = transition.active;
            let generation = {
                let memo = inner.memos.existing_mut(index).expect("prepared Memo");
                memo.advance(expected)?;
                memo.generation()
            };
            self.changed();
            accepted(generation);
            return Ok(());
        }
        let page = page.expect("retained publication Page");
        source.same_page(&page)?;
        let change = crate::basis::cell::change::acquire(&page)?;
        let flight = source.begin_cpu(self.identity(), access, maintenance_owner.as_deref())?;
        let lease = range.reside_during(&change)?;
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
            if !admitted() {
                return Err(AccessError::Denied);
            }
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
        let writable = access.contains(Access::WRITE);
        let admission;
        let result = {
            let mut address = nest.address.lock();
            let mut state = self.translation.lock();
            if !state.retirement.invalidation().is_clean() {
                return Err(AccessError::Busy);
            }
            if !state.is_open() || !self.is_open() {
                return Err(AccessError::Denied);
            }
            {
                let inner = self.state.lock();
                let memo = inner.memos.get(index).ok_or(AccessError::Denied)?;
                if !memo.matches(&binding) {
                    return Err(AccessError::Busy);
                }
            }
            let mut operation_freeze = if retained.is_some() {
                Some(Freeze::new(change.clone(), None).map_err(NestError::from)?)
            } else {
                None
            };
            let replacement = if retained.is_some_and(|old| old != writable) {
                Some((
                    index,
                    Publication::prepare_cpu(
                        &page,
                        writable,
                        &change,
                        maintenance.as_ref().map(|u| u.permit()),
                        crate::metal::generic::memory::Endpoint::Cpu {
                            nest: core::num::NonZeroU64::new(nest.id.get() as u64).unwrap(),
                            leases: core::num::NonZeroU64::new(nest.id.get() as u64).unwrap(),
                        },
                        crate::metal::generic::memory::Access::new((access.bits() << 1) as u8)
                            .ok_or(AccessError::Denied)?,
                    )
                    .map_err(NestError::from)?,
                ))
            } else {
                None
            };
            // Admission and PTE edits share the checked retirement batch.
            admission = if retained.is_some() && replacement.is_none() {
                Some(
                    Publication::prepare_cpu(
                        &page,
                        writable,
                        &change,
                        maintenance.as_ref().map(|u| u.permit()),
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
            let selection = self.range(PageOffset::new(index), count)?;
            self.prepare_revision()?;
            state.protect(
                &mut address.index,
                nest.geometry(),
                self,
                selection.window(),
                super::Protection::SetAccess(access),
                replacement,
                &mut || {
                    self.changed();
                    accepted(expected.checked_add(1).expect("prepared generation"));
                },
            )?;
            self.state
                .lock()
                .memos
                .existing_mut(index)
                .expect("prepared Memo")
                .protect(expected, operation_freeze.take());
            if state.retirement.invalidation().is_clean() {
                Ok::<_, AccessError>(None)
            } else {
                state
                    .retirement
                    .begin(nest.id.get(), nest.asid())
                    .map(Some)
                    .map_err(AccessError::from)
            }
        };
        drop(commit);
        let result = result.and_then(|target| match target {
            Some(target) => nest.invalidate(target).map_err(AccessError::from),
            None => Ok(()),
        });
        drop(admission);
        let valid = {
            let _commit = crate::basis::commit();
            admitted()
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
            let _ = self.retract_forced(nest, index, change);
        }
        drop(instructions);
        drop(lease);
        drop(flight);
        sign::signal(WakeKey::Span {
            space: self.identity.nest_id(),
            span: self.identity(),
        });
        if valid {
            result
        } else {
            Err(AccessError::Denied)
        }
    }
}

impl Span {
    /// Retire this Span's exported source discovery before address reuse.
    pub(in crate::basis::nest) fn withdraw_exports(&self) -> Result<(), memo::AccessError> {
        memo::source::exports::withdraw(&self.exports, |source| source.uses_span(self))
            .map_err(memo::pend_failure)
    }
}

impl Span {
    /// Advance the local close drain by at most one position.
    ///
    /// The cursor moves only after a position is cleared, so a blocked page is
    /// retried by the next call instead of being skipped.
    pub(in crate::basis::nest) fn drain_one(
        self: &Arc<Self>,
        nest: &Nest,
    ) -> Result<DrainStep, memo::AccessError> {
        let cursor = self.state.lock().drain_cursor;
        let Some(index) = self.state.lock().memos.next_page(cursor) else {
            return Ok(DrainStep::Done);
        };
        self.finish_memo(index);
        let pending = self
            .state
            .lock()
            .memos
            .get(index)
            .is_some_and(memo::Memo::busy);
        if pending {
            let Some(page) = self.memo_page(index)? else {
                return Ok(DrainStep::Blocked);
            };
            let change =
                crate::basis::cell::change::acquire(&page).map_err(|_| memo::AccessError::Busy)?;
            self.retract_forced(nest, index, change)?;
        }
        let cleared = {
            let _commit = crate::basis::commit();
            self.state
                .lock()
                .memos
                .existing_mut(index)
                .expect("selected Memo")
                .clear()
                .map(|removed| {
                    self.changed();
                    removed
                })
        };
        match cleared {
            Ok(cleared) => {
                self.state.lock().drain_cursor = Some(index);
                cleared.release();
                Ok(DrainStep::Progress)
            }
            Err(_) => Ok(DrainStep::Blocked),
        }
    }
}

impl Span {
    /// Persistent source lookup never materializes or retains a physical Page.
    pub(crate) fn logical_source(
        &self,
        index: usize,
        expected: u64,
    ) -> Result<Option<Source>, NestError> {
        self.position(index)?;
        if self.source().is_some() {
            return Err(NestError::Pinned);
        }
        let state = self.state.lock();
        let Some(memo) = state.memos.get(index) else {
            return if expected == 1 {
                Ok(None)
            } else {
                Err(NestError::Busy)
            };
        };
        if memo.generation() != expected {
            return Err(NestError::Busy);
        }
        if !memo.accepts_logical() {
            return Err(NestError::Pinned);
        }
        Ok(memo.association().map(|(_, source, _)| source))
    }

    /// Snapshot one logical position with this Span's own identity and the
    /// shared Translation's failure state.
    pub(crate) fn inspect(
        &self,
        index: usize,
    ) -> Result<kernel_api::model::nest::MemoStatus, NestError> {
        self.position(index)?;
        if self.source().is_some() {
            return Err(NestError::Pinned);
        }
        let translation = self.translation.lock();
        let state = self.state.lock();
        let vacant = memo::Memo::vacant();
        let memo = state.memos.get(index).unwrap_or(&vacant);
        if !memo.accepts_logical() {
            return Err(NestError::Pinned);
        }
        Ok(memo.snapshot(
            self.identity.nest_id(),
            kernel_api::model::nest::SpanId::from_bytes(&(self.identity() as u64).to_le_bytes())
                .expect("span identity"),
            index,
            self.level(),
            translation.retirement.invalidation()
                == crate::basis::nest::translation::Invalidation::Failed,
        ))
    }
}

impl Span {
    /// Withdraw one published page and retain its exact Page responsibility.
    pub(crate) fn unpublish_page(
        self: &Arc<Self>,
        nest: &Nest,
        address: VirtAddr,
        expected: &Arc<Page>,
    ) -> Result<(), NestError> {
        self.unpublish_page_with(nest, address, expected, false)
    }

    pub(in crate::basis::nest) fn unpublish_page_with(
        self: &Arc<Self>,
        nest: &Nest,
        address: VirtAddr,
        expected: &Arc<Page>,
        polling: bool,
    ) -> Result<(), NestError> {
        if nest.asid().is_kernel() || self.is_global() {
            return Err(NestError::Pinned);
        }
        assert!(
            address.offset() == 0,
            "unpublish_page takes a page-aligned address"
        );
        assert!(
            self.identity.belongs_to(nest.id),
            "unpublish_page requires the mapping's own live nest"
        );
        nest.finish_invalidation_with(polling)?;

        let mut state = self.translation.lock();
        if !state.retirement.invalidation().is_clean() {
            return Err(NestError::Busy);
        }
        if !state.unpublish_page(nest.geometry(), self, address, expected)? {
            return Ok(());
        }
        let target = state.retirement.begin(nest.id.get(), nest.asid())?;
        drop(state);
        // Release retained resources only after this exact invocation completes.
        nest.start_invalidation(target, polling)?;
        self.finish_memo(self.page_index(address));
        Ok(())
    }
}

impl Span {
    /// Sample phase and revision under the same owner lock as physical completion.
    pub(crate) fn snapshot(
        &self,
        nest: &Nest,
        offset: kernel_api::address::PageOffset,
        count: kernel_api::address::PageCount,
        writable: bool,
    ) -> Result<kernel_api::model::nest::SpanStatus, NestError> {
        use super::SpanPhase as LocalPhase;
        use kernel_api::model::nest::{SpanId, SpanPhase, SpanStatus};
        nest.check_owner(self)?;
        let translation = self.translation.lock();
        let inner = self.state.lock();
        let phase = match inner.phase {
            LocalPhase::Open if translation.is_open() => SpanPhase::Open,
            LocalPhase::Gone => SpanPhase::Closed,
            _ => SpanPhase::Closing,
        };
        let mut access = inner.initial_access;
        if !writable {
            access.remove(Access::WRITE);
        }
        Ok(SpanStatus {
            nest: self.identity.nest_id(),
            span: SpanId::from_bytes(&(self.identity() as u64).to_le_bytes())
                .expect("span identity"),
            version: self.version(),
            offset: offset.get(),
            count: count.get(),
            address: self.initial_start(),
            level: self.level(),
            access: access.bits() << 1,
            phase,
        })
    }
}

impl Span {
    /// Withdraw the published positions in one authority window.
    pub(crate) fn withdraw(
        self: &Arc<Self>,
        nest: &Nest,
        window: (
            kernel_api::address::PageOffset,
            kernel_api::address::PageCount,
        ),
        authority: &Arc<crate::basis::pend::Pend>,
    ) -> Result<(), kernel_api::abi::call::pend::PendFail> {
        use kernel_api::abi::call::pend::PendFail;
        use kernel_api::model::pend::{NestScope, PendScope};
        let _operation = self.operation().ok_or(PendFail::Busy)?;
        self.range(window.0, window.1)
            .map_err(|_| PendFail::NotAligned)?;
        if !matches!(authority.scope(), PendScope::Nest(NestScope::Span { id: known, .. }) if known.get() == self.identity())
        {
            return Err(PendFail::Denied);
        }
        if !nest.is_external() {
            nest.finish_invalidation().map_err(|_| PendFail::Busy)?;
        }
        let end = window
            .0
            .get()
            .checked_add(window.1.get())
            .ok_or(PendFail::NotAligned)?;
        let mut cursor = None;
        let mut busy = false;
        loop {
            let Some(index) = self.state.lock().memos.next_page(cursor) else {
                break;
            };
            cursor = Some(index);
            if index < window.0.get() || index >= end {
                continue;
            }
            self.finish_memo(index);
            if !self.memo_published_by(index, authority) {
                continue;
            }
            let Some(page) = self.memo_page(index).map_err(|_| PendFail::Busy)? else {
                busy = true;
                continue;
            };
            match crate::basis::cell::change::acquire(&page) {
                Ok(change) => busy |= self.retract_forced(nest, index, change).is_err(),
                Err(_) => busy = true,
            }
        }
        sign::signal(WakeKey::Span {
            space: self.identity.nest_id(),
            span: self.identity(),
        });
        if nest.is_external() {
            sign::signal(WakeKey::ExternalSpan {
                id: self.identity(),
            });
        }
        if busy { Err(PendFail::Busy) } else { Ok(()) }
    }
}

impl Span {
    /// Release a page-aligned subrange of this Span's active access and
    /// occupancy. A released hole is immediately reusable by a new Span while
    /// this historical Span retains its original identity and remaining ranges.
    pub(in crate::basis::nest) fn release_range(
        self: &Arc<Self>,
        nest: &Nest,
        offset: kernel_api::address::PageOffset,
        count: kernel_api::address::PageCount,
    ) -> Result<(), NestError> {
        let start = VirtAddr::new(
            self.initial_start()
                .as_usize()
                .checked_add(
                    offset
                        .get()
                        .checked_mul(kernel_api::address::PAGE_SIZE)
                        .ok_or(NestError::NotAligned)?,
                )
                .ok_or(NestError::NotAligned)?,
        );
        let last = start
            .as_usize()
            .checked_add(count.checked_bytes().ok_or(NestError::NotAligned)?)
            .and_then(|end| end.checked_sub(1))
            .ok_or(NestError::NotAligned)?;
        let pages = self.initial_size() / kernel_api::address::PAGE_SIZE;
        if offset
            .get()
            .checked_add(count.get())
            .is_none_or(|end| end > pages)
        {
            return Err(NestError::NotAligned);
        }
        let mut address = nest.address.lock();
        let translation = self.translation.lock();
        let mut inner = self.state.lock();
        if inner.phase != super::SpanPhase::Open
            || inner.source.is_some()
            || translation.retirement.pending_for(self)
        {
            return Err(NestError::Busy);
        }
        if inner.memos.published().any(|(page, _)| {
            let at = self.initial_start().as_usize() + page * kernel_api::address::PAGE_SIZE;
            (start.as_usize()..=last).contains(&at)
        }) {
            return Err(NestError::Busy);
        }
        let mut cursor = None;
        while let Some(page) = inner.memos.next_page(cursor) {
            cursor = Some(page);
            if page >= offset.get() && page < offset.get() + count.get() {
                if let Some(memo) = inner.memos.get(page)
                    && (memo.in_flight() || memo.busy())
                {
                    return Err(NestError::Busy);
                }
            }
        }
        let next = super::ActiveRange::split(&inner.active, start.as_usize(), last, None)?;
        if next.len() == inner.active.len() {
            return Err(NestError::NoRegion);
        }
        if next.is_empty() {
            return Err(NestError::Busy);
        }
        if !address
            .occupancy
            .release_range(start.as_usize(), last, self.identity())
        {
            return Err(NestError::Busy);
        }
        inner.active = next;
        self.changed();
        drop(inner);
        assert!(address.index.refresh(self), "released Span is indexed");
        let _ = translation;
        Ok(())
    }
}

impl Span {
    pub(crate) fn count(&self) -> Option<kernel_api::address::PageCount> {
        kernel_api::address::PageCount::new(self.initial_size() / kernel_api::address::PAGE_SIZE)
    }
}

impl Span {
    /// Materialize Flat source pages for one selection and publish each active
    /// segment. This composes the existing Flat primitive with the Span's own
    /// publication; it is not a second materialization authority.
    pub(crate) fn materialize_active(
        self: &Arc<Self>,
        nest: &Arc<Nest>,
        selection: &NestRange,
    ) -> Result<(), NestError> {
        if !Arc::ptr_eq(selection.span(), self) {
            return Err(NestError::NoRegion);
        }
        for (start, size) in self.active_segments(nest, selection)? {
            let offset = PageOffset::new(self.page_index(start));
            let count = PageCount::new(size / kernel_api::address::PAGE_SIZE)
                .ok_or(NestError::NotAligned)?;
            let part = self.range(offset, count)?;
            part.source()?.materialize()?;
            self.publish_segments(nest, &part)?;
        }
        Ok(())
    }

    /// Active pages clipped to the selection, as canonical start/size pairs.
    pub(in crate::basis::nest) fn active_segments(
        self: &Arc<Self>,
        nest: &Arc<Nest>,
        selection: &NestRange,
    ) -> Result<Vec<(VirtAddr, usize)>, NestError> {
        let (lo, hi) = selection.window();
        let state = nest.ready_state()?;
        nest.ensure_owned(self, &state)?;
        let inner = self.state.lock();
        if inner.active.is_empty()
            || (matches!(selection, NestRange::Pages { .. }) && !inner.covers(lo, hi))
        {
            return Err(NestError::NoRegion);
        }
        let mut segments = Vec::new();
        segments
            .try_reserve(inner.active.len())
            .map_err(|_| NestError::OutOfMemory)?;
        for range in &inner.active {
            if let Some((start, last)) = range.intersection(lo, hi) {
                segments.push((
                    crate::metal::mmu::internal::canonical(start),
                    last - start + 1,
                ));
            }
        }
        Ok(segments)
    }
}

impl Span {
    /// Publish one active segment from the Span's implicit source. This is the
    /// hardware publication step for materialized pages; Nest supplies the
    /// ready state, translation root and invalidation progress.
    pub(crate) fn publish_segment(
        self: &Arc<Self>,
        nest: &Arc<Nest>,
        start: VirtAddr,
        size: usize,
    ) -> Result<(), NestError> {
        if size == 0 || start.offset() != 0 || !size.is_multiple_of(kernel_api::address::PAGE_SIZE)
        {
            return Err(NestError::NotAligned);
        }
        let end = start
            .as_usize()
            .checked_add(size - 1)
            .ok_or(NestError::NotAligned)?;
        let mut state = nest.ready_state()?;
        nest.ensure_owned(self, &state)?;
        let mut inner = self.state.lock();
        if !inner.covers(start.as_usize(), end) {
            return Err(NestError::NoRegion);
        }
        let source = inner.source.as_ref().ok_or(NestError::Missing)?;
        if source.is_borrowed() {
            return Ok(());
        }
        let first = self.page_index(start);
        let window = source.range(
            PageOffset::new(first),
            PageCount::new(size / kernel_api::address::PAGE_SIZE).ok_or(NestError::NotAligned)?,
        )?;
        // Hold the source for the whole PTE publication; an in-flight handoff
        // keeps this Busy before any page-table write.
        let lease = window.reside()?;
        let materialized = window.materialized_cells()?;
        let mut fresh = Vec::new();
        fresh
            .try_reserve(materialized.len())
            .map_err(|_| NestError::OutOfMemory)?;
        for (offset, cell) in materialized {
            let page = first + offset.get();
            if inner.memos.publication(page).is_none() {
                let address = self.range.start() + page * kernel_api::address::PAGE_SIZE;
                let range = inner
                    .active
                    .iter()
                    .find(|range| range.contains(address.as_usize()))
                    .ok_or(NestError::NoRegion)?;
                fresh.push((
                    page,
                    Publication::prepare(&cell, range.ceiling.contains(Access::WRITE), None)?,
                ));
            }
        }
        if fresh.is_empty() {
            return Ok(());
        }
        inner.memos.reserve(fresh.len())?;
        let source = inner.source.as_ref().ok_or(NestError::Missing)?;
        state
            .retirement
            .mappings
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;
        for run in inner.runs(self.initial_start(), &fresh) {
            let run = run?;
            state.root.cpu_mut()?.prepare_range_geometry(
                run.start,
                run.physical,
                run.size,
                nest.geometry(),
            )?;
        }
        if nest.asid().is_kernel() || self.is_global() {
            source.mark_untransferable();
        }
        for (_, retained) in &mut fresh {
            retained.publish();
        }
        state.retirement.mappings.push(Arc::downgrade(self));
        for run in inner.runs(self.initial_start(), &fresh) {
            let run = run?;
            state.root.cpu_mut()?.map_range_prepared_geometry(
                run.start,
                run.physical,
                run.size,
                self.leaf_attributes(run.current),
                nest.geometry(),
            );
        }
        for (page, cell) in fresh {
            inner.memos.publish(page, cell);
        }
        state.retirement.mark_dirty(self.is_global());
        drop(inner);
        drop(state);
        drop(lease);
        nest.finish_invalidation()
    }
}

impl Span {
    /// Pend-visible lifecycle. Nest supplies its admission state so a closing
    /// space retires even while the Span's own phase is still Open.
    pub(crate) fn pend_status(&self, nest_open: bool) -> kernel_api::model::pend::PendStatus {
        use super::SpanPhase;
        use kernel_api::model::pend::PendStatus;
        match self.state.lock().phase {
            SpanPhase::Open if nest_open => PendStatus::Active,
            SpanPhase::Open | SpanPhase::Closing => PendStatus::Retiring,
            SpanPhase::Gone => PendStatus::Dead,
        }
    }
}

impl Span {
    /// Withdraw every published local Memo. Used by source withdrawal after
    /// the source authorization has been selected.
    pub(in crate::basis::nest) fn unpublish_all(
        self: &Arc<Self>,
        nest: &Arc<Nest>,
    ) -> Result<(), kernel_api::abi::call::pend::PendFail> {
        use kernel_api::abi::call::pend::PendFail;
        let mut cursor = None;
        loop {
            let Some(index) = self.state.lock().memos.next_page(cursor) else {
                break;
            };
            cursor = Some(index);
            let Some(page) = self.memo_page(index).map_err(|_| PendFail::Busy)? else {
                continue;
            };
            self.unpublish_page(
                nest,
                VirtAddr::new(self.position(index).map_err(|_| PendFail::Busy)?),
                &page,
            )
            .map_err(|_| PendFail::Busy)?;
            self.finish_memo(index);
        }
        Ok(())
    }
}

impl Span {
    /// Publish each active segment of a selection from this Span.
    pub(crate) fn publish_segments(
        self: &Arc<Self>,
        nest: &Arc<Nest>,
        selection: &NestRange,
    ) -> Result<(), NestError> {
        for (start, size) in self.active_segments(nest, selection)? {
            self.publish_segment(nest, start, size)?;
        }
        Ok(())
    }
}
