//! The Nest's span namespace and shared source-discovery bindings.

pub(crate) mod admission;
pub(crate) mod binding;
pub(crate) mod external;
mod source;
pub(crate) use source::{withdraw_source, withdraw_source_branch, withdraw_source_window};

use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;

use super::{Nest, Span};
use crate::basis::pend::{Pend, SourceAuthorization};
use crate::metal::sync::SpinLock;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::model::identity::TaskId;
use kernel_api::model::pend::PendStatus;

/// A discovery identity, shared verbatim with its source membership.
pub(crate) struct RangeBinding {
    id: usize,
    nest: Weak<Nest>,
    page: Option<usize>,
}

impl RangeBinding {
    pub(crate) fn new(id: usize, nest: &Arc<Nest>) -> Result<Arc<Self>, PendFail> {
        Self::build(id, nest, None)
    }

    pub(crate) fn new_position(
        id: usize,
        nest: &Arc<Nest>,
        page: usize,
    ) -> Result<Arc<Self>, PendFail> {
        Self::build(id, nest, Some(page))
    }

    fn build(id: usize, nest: &Arc<Nest>, page: Option<usize>) -> Result<Arc<Self>, PendFail> {
        Arc::try_new(Self {
            id,
            nest: Arc::downgrade(nest),
            page,
        })
        .map_err(|_| PendFail::OutOfMemory)
    }

    pub(crate) fn id(&self) -> usize {
        self.id
    }

    pub(crate) fn nest(&self) -> &Weak<Nest> {
        &self.nest
    }

    pub(crate) fn page(&self) -> Option<usize> {
        self.page
    }
}

#[derive(Clone)]
pub(in crate::basis::nest) struct MappingRoot {
    pub(in crate::basis::nest) creator: TaskId,
    pub(in crate::basis::nest) authority: Weak<Pend>,
}

#[derive(Clone)]
pub(crate) struct RangeEntry {
    pub(crate) binding: Arc<RangeBinding>,
    pub(crate) mapping: Option<Arc<Span>>,
    source: Option<Arc<SourceAuthorization>>,
    pub(in crate::basis::nest) root: Option<MappingRoot>,
}

impl RangeEntry {
    pub(crate) fn mapping(&self) -> Option<&Arc<Span>> {
        self.mapping.as_ref()
    }

    pub(crate) fn source(&self) -> Option<&Arc<SourceAuthorization>> {
        self.source.as_ref()
    }

    fn reapable(&self, nest: &Nest) -> bool {
        nest.settled(self.binding.id())
            && self.root.as_ref().is_none_or(|root| {
                self.mapping().is_some_and(|span| {
                    span.state.lock().phase == super::address::span::MappingPhase::Gone
                }) && root
                    .authority
                    .upgrade()
                    .is_none_or(|authority| authority.status() == PendStatus::Dead)
            })
    }
}

struct Ranges {
    entries: Vec<RangeEntry>,
    /// Private staging owns capacity, not a visible mapping or root.
    prepared: usize,
    cursor: usize,
}

pub(crate) struct RangeRegistry {
    ranges: SpinLock<Ranges>,
}

impl RangeRegistry {
    pub(crate) fn new() -> Self {
        Self {
            ranges: SpinLock::new(Ranges {
                entries: Vec::new(),
                prepared: 0,
                cursor: 0,
            }),
        }
    }

    pub(crate) fn claim_mapping(
        &self,
        id: usize,
        nest: &Arc<Nest>,
        source: Arc<SourceAuthorization>,
    ) -> Result<(), PendFail> {
        let _commit = crate::basis::commit();
        let binding = RangeBinding::new(id, nest)?;
        {
            let mut ranges = self.ranges.lock();
            if ranges.entries.iter().any(|entry| entry.binding.id() == id) {
                return Err(PendFail::Busy);
            }
            let capacity = ranges
                .prepared
                .checked_add(1)
                .ok_or(PendFail::OutOfMemory)?;
            ranges
                .entries
                .try_reserve(capacity)
                .map_err(|_| PendFail::OutOfMemory)?;
        }
        source.reserve_binding()?;
        source.register_binding(binding.clone());
        self.ranges.lock().entries.push(RangeEntry {
            binding,
            mapping: None,
            source: Some(source),
            root: None,
        });
        Ok(())
    }

    pub(crate) fn commit(&self, id: usize, mapping: Arc<Span>) {
        let mut ranges = self.ranges.lock();
        let entry = ranges
            .entries
            .iter_mut()
            .find(|entry| entry.binding.id() == id)
            .expect("prepared implicit mapping registration");
        assert!(entry.root.is_none());
        entry.mapping = Some(mapping);
    }

    pub(in crate::basis::nest) fn prepare(&self) -> Result<(), PendFail> {
        let mut ranges = self.ranges.lock();
        let prepared = ranges
            .prepared
            .checked_add(1)
            .ok_or(PendFail::OutOfMemory)?;
        ranges
            .entries
            .try_reserve(prepared)
            .map_err(|_| PendFail::OutOfMemory)?;
        ranges.prepared = prepared;
        Ok(())
    }

    pub(in crate::basis::nest) fn cancel_prepared(&self) {
        let mut ranges = self.ranges.lock();
        ranges.prepared = ranges.prepared.checked_sub(1).expect("prepared range");
    }

    pub(in crate::basis::nest) fn publish(
        &self,
        binding: Arc<RangeBinding>,
        mapping: Arc<Span>,
        creator: TaskId,
        root: &Arc<Pend>,
    ) {
        let mut ranges = self.ranges.lock();
        assert!(ranges.prepared != 0 && ranges.entries.len() < ranges.entries.capacity());
        assert!(
            !ranges
                .entries
                .iter()
                .any(|entry| entry.binding.id() == binding.id())
        );
        ranges.prepared -= 1;
        ranges.entries.push(RangeEntry {
            binding,
            mapping: Some(mapping),
            source: None,
            root: Some(MappingRoot {
                creator,
                authority: Arc::downgrade(root),
            }),
        });
    }

    pub(crate) fn registered(&self, id: usize) -> Option<RangeEntry> {
        self.ranges
            .lock()
            .entries
            .iter()
            .find(|entry| entry.binding.id() == id)
            .cloned()
    }

    pub(in crate::basis::nest) fn next(&self, after: usize) -> Option<RangeEntry> {
        self.ranges
            .lock()
            .entries
            .iter()
            .filter(|entry| entry.binding.id() > after)
            .min_by_key(|entry| entry.binding.id())
            .cloned()
    }

    pub(in crate::basis::nest) fn progress_entry(&self) -> Option<RangeEntry> {
        let mut ranges = self.ranges.lock();
        let entry = ranges
            .entries
            .iter()
            .filter(|entry| entry.root.is_some() && entry.binding.id() > ranges.cursor)
            .min_by_key(|entry| entry.binding.id())
            .or_else(|| {
                ranges
                    .entries
                    .iter()
                    .filter(|entry| entry.root.is_some())
                    .min_by_key(|entry| entry.binding.id())
            })
            .cloned()?;
        ranges.cursor = entry.binding.id();
        Some(entry)
    }

    pub(in crate::basis::nest) fn empty(&self) -> bool {
        let ranges = self.ranges.lock();
        ranges.entries.is_empty() && ranges.prepared == 0
    }

    pub(in crate::basis::nest) fn competing_reservation(&self, creator: TaskId) -> bool {
        let mut cursor = 0;
        while let Some(entry) = self.next(cursor) {
            cursor = entry.binding.id();
            if entry
                .root
                .as_ref()
                .is_some_and(|root| root.creator != creator)
                && entry.mapping().is_some_and(|span| {
                    span.state.lock().phase != super::address::span::MappingPhase::Gone
                })
            {
                return true;
            }
        }
        false
    }

    pub(crate) fn retire(&self, id: usize) -> Option<RangeEntry> {
        let mut ranges = self.ranges.lock();
        let at = ranges
            .entries
            .iter()
            .position(|entry| entry.binding.id() == id)?;
        Some(ranges.entries.remove(at))
    }

    pub(crate) fn take_settled(&self, nest: &Nest) -> Option<RangeEntry> {
        let mut cursor = 0;
        while let Some(entry) = self.next(cursor) {
            cursor = entry.binding.id();
            // Upgrade/drop authority and Span references outside the registry lock.
            if entry.reapable(nest) {
                return self.retire(cursor);
            }
        }
        None
    }

    pub(crate) fn owned_by(
        &self,
        source: &Arc<SourceAuthorization>,
    ) -> Result<Vec<RangeEntry>, PendFail> {
        let ranges = self.ranges.lock();
        let mut result = Vec::new();
        result
            .try_reserve(ranges.entries.len())
            .map_err(|_| PendFail::OutOfMemory)?;
        result.extend(
            ranges
                .entries
                .iter()
                .filter(|entry| {
                    entry.mapping.is_some()
                        && entry.source().is_some_and(|held| Arc::ptr_eq(held, source))
                })
                .cloned(),
        );
        Ok(result)
    }
}

// PendId is observation metadata in SourceAuthorization, never a transferable handle here.
unsafe impl Send for RangeRegistry {}
unsafe impl Sync for RangeRegistry {}

impl Nest {
    fn settle(&self, entry: RangeEntry) {
        if let Some(source) = entry.source() {
            source.release_binding(&entry.binding);
        }
        if entry.root.is_some()
            && let Some(mapping) = entry.mapping()
        {
            loop {
                let memo = self.state.lock().memos.take_metadata(mapping);
                if memo.is_none() {
                    break;
                }
                drop(memo);
            }
        }
    }

    pub(crate) fn retire_mapping(&self, id: usize) {
        let _commit = crate::basis::commit();
        let Some(entry) = self.mappings.registered(id) else {
            return;
        };
        if entry.reapable(self)
            && let Some(entry) = self.mappings.retire(id)
        {
            self.settle(entry);
        }
    }

    /// The whole-Nest keeper advances a bounded number of the real namespace records.
    pub(crate) fn progress_mappings(self: &Arc<Self>) {
        use kernel_api::model::cell::MappingId;
        if !self.is_external() {
            return;
        }
        for _ in 0..4 {
            let _commit = crate::basis::commit();
            let Some(entry) = self.mappings.progress_entry() else {
                break;
            };
            let Some(mapping) = entry.mapping() else {
                continue;
            };
            let Some(root) = entry.root.as_ref() else {
                continue;
            };
            let Some(id) = MappingId::from_bytes(&(mapping.identity() as u64).to_le_bytes()) else {
                continue;
            };
            let lost = root.authority.upgrade().is_none_or(|root| !root.alive());
            if lost || !self.is_open() || !mapping.is_open() {
                let _ = self.release_mapping(id);
                self.retire_mapping(id.get());
                continue;
            }
            let snapshot = {
                let state = self.state.lock();
                state
                    .memos
                    .get(mapping, 0)
                    .map(|memo| (memo.association(), memo.principal()))
            };
            let Some((association, principal)) = snapshot else {
                continue;
            };
            if let Some((_, source, binding)) = association {
                if source.retiring() {
                    let _ = self.withdraw_source(&binding, source.source(), None, None, true);
                    continue;
                }
            }
            if let Some(principal) = principal
                && mapping.access(0).map_or(true, |(_, access)| {
                    principal.check(self, mapping, 0, access).is_err()
                })
            {
                if let Some(count) = kernel_api::address::PageCount::new(mapping.grain()) {
                    let _ = self.withdraw_mapping(
                        id,
                        (kernel_api::address::PageOffset::ZERO, count),
                        principal.authority(),
                    );
                }
            } else if let Some(_operation) = mapping.operation() {
                let before = self.inspect_memo(mapping, 0).ok();
                let _ = self.progress_external(mapping);
                if before != self.inspect_memo(mapping, 0).ok() {
                    crate::basis::sign::signal(crate::basis::sign::WakeKey::ExternalMapping {
                        id: id.get(),
                    });
                }
            }
        }
    }

    pub(in crate::basis::nest) fn mappings_gone(&self) -> bool {
        let mut cursor = 0;
        while let Some(entry) = self.mappings.next(cursor) {
            cursor = entry.binding.id();
            if entry.root.is_some()
                && entry.mapping().is_some_and(|mapping| {
                    mapping.state.lock().phase != super::address::span::MappingPhase::Gone
                })
            {
                return false;
            }
        }
        true
    }

    pub(crate) fn reap_mappings(&self) -> bool {
        for _ in 0..4 {
            let _commit = crate::basis::commit();
            let Some(entry) = self.mappings.take_settled(self) else {
                break;
            };
            self.settle(entry);
        }
        let mut cursor = 0;
        while let Some(entry) = self.mappings.next(cursor) {
            cursor = entry.binding.id();
            if !self.settled(cursor) {
                return false;
            }
        }
        true
    }
}
