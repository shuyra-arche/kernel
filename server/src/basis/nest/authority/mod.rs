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
pub(in crate::basis::nest) struct ReservationRoot {
    pub(in crate::basis::nest) creator: TaskId,
    pub(in crate::basis::nest) authority: Weak<Pend>,
}

#[derive(Clone)]
pub(crate) struct RangeEntry {
    pub(crate) binding: Arc<RangeBinding>,
    pub(crate) reservation: Option<Arc<Span>>,
    source: Option<Arc<SourceAuthorization>>,
    pub(in crate::basis::nest) root: Option<ReservationRoot>,
}

impl RangeEntry {
    pub(crate) fn reservation(&self) -> Option<&Arc<Span>> {
        self.reservation.as_ref()
    }

    pub(crate) fn source(&self) -> Option<&Arc<SourceAuthorization>> {
        self.source.as_ref()
    }

    fn reapable(&self, nest: &Nest) -> bool {
        nest.settled(self.binding.id())
            && self.root.as_ref().is_none_or(|root| {
                self.reservation().is_some_and(|span| {
                    span.state.lock().phase == super::address::span::ReservationPhase::Gone
                }) && root
                    .authority
                    .upgrade()
                    .is_none_or(|authority| authority.status() == PendStatus::Dead)
            })
    }
}

struct Ranges {
    entries: Vec<RangeEntry>,
    /// Private staging owns capacity, not a visible reservation or root.
    prepared: usize,
    cursor: usize,
}

pub(crate) struct ReservationRegistry {
    ranges: SpinLock<Ranges>,
}

impl ReservationRegistry {
    pub(crate) fn new() -> Self {
        Self {
            ranges: SpinLock::new(Ranges {
                entries: Vec::new(),
                prepared: 0,
                cursor: 0,
            }),
        }
    }

    pub(crate) fn claim_reservation(
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
            reservation: None,
            source: Some(source),
            root: None,
        });
        Ok(())
    }

    pub(crate) fn commit(&self, id: usize, reservation: Arc<Span>) {
        let mut ranges = self.ranges.lock();
        let entry = ranges
            .entries
            .iter_mut()
            .find(|entry| entry.binding.id() == id)
            .expect("prepared implicit reservation registration");
        assert!(entry.root.is_none());
        entry.reservation = Some(reservation);
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
        reservation: Arc<Span>,
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
            reservation: Some(reservation),
            source: None,
            root: Some(ReservationRoot {
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
                && entry.reservation().is_some_and(|span| {
                    span.state.lock().phase != super::address::span::ReservationPhase::Gone
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
}

// PendId is observation metadata in SourceAuthorization, never a transferable handle here.
unsafe impl Send for ReservationRegistry {}
unsafe impl Sync for ReservationRegistry {}

impl Nest {
    fn settle(&self, entry: RangeEntry) {
        if let Some(source) = entry.source() {
            source.release_binding(&entry.binding);
        }
        if entry.root.is_some()
            && let Some(reservation) = entry.reservation()
        {
            loop {
                let memo = self.state.lock().memos.take_metadata(reservation);
                if memo.is_none() {
                    break;
                }
                drop(memo);
            }
        }
    }

    pub(crate) fn retire_reservation(&self, id: usize) {
        let _commit = crate::basis::commit();
        let Some(entry) = self.reservations.registered(id) else {
            return;
        };
        if entry.reapable(self)
            && let Some(entry) = self.reservations.retire(id)
        {
            self.settle(entry);
        }
    }

    /// The whole-Nest keeper advances a bounded number of the real namespace records.
    pub(crate) fn progress_reservations(self: &Arc<Self>) {
        use kernel_api::model::nest::ReservationId;
        if !self.is_external() {
            return;
        }
        for _ in 0..4 {
            let _commit = crate::basis::commit();
            let Some(entry) = self.reservations.progress_entry() else {
                break;
            };
            let Some(reservation) = entry.reservation() else {
                continue;
            };
            let Some(root) = entry.root.as_ref() else {
                continue;
            };
            let Some(id) =
                ReservationId::from_bytes(&(reservation.identity() as u64).to_le_bytes())
            else {
                continue;
            };
            let lost = root.authority.upgrade().is_none_or(|root| !root.alive());
            if lost || !self.is_open() || !reservation.is_open() {
                let _ = self.release_reservation(id);
                self.retire_reservation(id.get());
                continue;
            }
            let snapshot = {
                let state = self.state.lock();
                state
                    .memos
                    .get(reservation, 0)
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
                && reservation.access(0).map_or(true, |(_, access)| {
                    principal.check(self, reservation, 0, access).is_err()
                })
            {
                if let Some(count) = kernel_api::address::PageCount::new(reservation.grain()) {
                    let _ = self.withdraw_reservation(
                        id,
                        (kernel_api::address::PageOffset::ZERO, count),
                        principal.authority(),
                    );
                }
            } else if let Some(_operation) = reservation.operation() {
                let before = self.inspect_memo(reservation, 0).ok();
                let _ = self.progress_external(reservation);
                if before != self.inspect_memo(reservation, 0).ok() {
                    crate::basis::sign::signal(crate::basis::sign::WakeKey::ExternalReservation {
                        id: id.get(),
                    });
                }
            }
        }
    }

    /// CPU shutdown already proved every PTE absent; finish bounded namespace resources.
    pub(in crate::basis::nest) fn drain_reservation_resources(
        &self,
    ) -> Result<(), super::NestError> {
        if !self.is_closed() {
            return Err(super::NestError::Busy);
        }
        for _ in 0..4 {
            let Some(entry) = self.reservations.progress_entry() else {
                break;
            };
            let Some(reservation) = entry.reservation() else {
                continue;
            };
            if reservation.state.lock().phase == super::address::span::ReservationPhase::Gone {
                continue;
            }
            let id = kernel_api::model::nest::ReservationId::from_bytes(
                &(reservation.identity() as u64).to_le_bytes(),
            )
            .ok_or(super::NestError::NoRegion)?;
            self.release_reservation(id)
                .map_err(super::memo::source_error)?;
        }
        if self.reservations_gone() {
            Ok(())
        } else {
            Err(super::NestError::Busy)
        }
    }

    pub(in crate::basis::nest) fn reservations_gone(&self) -> bool {
        let mut cursor = 0;
        while let Some(entry) = self.reservations.next(cursor) {
            cursor = entry.binding.id();
            if entry.root.is_some()
                && entry.reservation().is_some_and(|reservation| {
                    reservation.state.lock().phase != super::address::span::ReservationPhase::Gone
                })
            {
                return false;
            }
        }
        true
    }

    pub(crate) fn reap_reservations(&self) -> bool {
        for _ in 0..4 {
            let _commit = crate::basis::commit();
            let Some(entry) = self.reservations.take_settled(self) else {
                break;
            };
            self.settle(entry);
        }
        let mut cursor = 0;
        while let Some(entry) = self.reservations.next(cursor) {
            cursor = entry.binding.id();
            if !self.settled(cursor) {
                return false;
            }
        }
        true
    }
}
