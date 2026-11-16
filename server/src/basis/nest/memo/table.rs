//! Sparse positions owned by one Span, keyed by page coordinate.
//! Address reuse cannot recover a former span's Memo.
use super::{Memo, publication::Publication};
use crate::basis::nest::{ActiveRange, NestError};
use alloc::vec::Vec;
use kernel_api::address::{PAGE_SIZE, VirtAddr};

pub(in crate::basis::nest) struct Memos {
    entries: Vec<(usize, Memo)>,
}
impl Memos {
    pub(in crate::basis::nest) fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
    pub(in crate::basis::nest) fn idle(&self) -> bool {
        self.entries.iter().all(|(_, memo)| !memo.busy())
    }
    pub(in crate::basis::nest) fn reserve(&mut self, additional: usize) -> Result<(), NestError> {
        self.entries
            .try_reserve(additional)
            .map_err(|_| NestError::OutOfMemory)
    }
    pub(in crate::basis::nest) fn get(&self, page: usize) -> Option<&Memo> {
        self.entries
            .binary_search_by_key(&page, |(known, _)| *known)
            .ok()
            .map(|at| &self.entries[at].1)
    }
    pub(in crate::basis::nest) fn get_mut(&mut self, page: usize) -> Result<&mut Memo, NestError> {
        let at = match self
            .entries
            .binary_search_by_key(&page, |(known, _)| *known)
        {
            Ok(at) => at,
            Err(at) => {
                self.reserve(1)?;
                self.entries.insert(at, (page, Memo::vacant()));
                at
            }
        };
        Ok(&mut self.entries[at].1)
    }
    pub(in crate::basis::nest) fn existing_mut(&mut self, page: usize) -> Option<&mut Memo> {
        let at = self
            .entries
            .binary_search_by_key(&page, |(known, _)| *known)
            .ok()?;
        Some(&mut self.entries[at].1)
    }
    pub(in crate::basis::nest) fn next_page(&self, after: Option<usize>) -> Option<usize> {
        self.entries
            .iter()
            .map(|(page, _)| *page)
            .find(|page| after.is_none_or(|after| *page > after))
    }
    pub(in crate::basis::nest) fn publication(&self, page: usize) -> Option<&Publication> {
        self.get(page).and_then(Memo::publication)
    }
    pub(in crate::basis::nest) fn published<'a>(
        &'a self,
    ) -> impl Iterator<Item = (usize, &'a Publication)> {
        self.entries
            .iter()
            .filter_map(|(page, memo)| memo.publication().map(|p| (*page, p)))
    }
    pub(in crate::basis::nest) fn publish(&mut self, page: usize, publication: Publication) {
        let at = match self
            .entries
            .binary_search_by_key(&page, |(known, _)| *known)
        {
            Ok(at) => at,
            Err(at) => {
                assert!(
                    self.entries.len() < self.entries.capacity(),
                    "prepare Memo before hardware publication"
                );
                self.entries.insert(at, (page, Memo::vacant()));
                at
            }
        };
        self.entries[at].1.install(publication);
    }
    pub(in crate::basis::nest) fn take_publication(&mut self, page: usize) -> Option<Publication> {
        let at = self
            .entries
            .binary_search_by_key(&page, |(known, _)| *known)
            .ok()?;
        self.entries[at].1.take_publication()
    }
    pub(in crate::basis::nest) fn withdraw(
        &mut self,
        first: usize,
        last: usize,
        retired: &mut Vec<Publication>,
    ) -> bool {
        let mut changed = false;
        for (page, memo) in &mut self.entries {
            if *page >= first && *page <= last {
                if let Some(publication) = memo.withdraw() {
                    retired.push(publication);
                    changed = true;
                }
            }
        }
        changed
    }
    pub(in crate::basis::nest) fn awaiting_cpu(&self) -> usize {
        self.entries
            .iter()
            .filter(|(_, memo)| memo.awaiting_cpu())
            .count()
    }
    pub(in crate::basis::nest) fn complete(&mut self, settled: &mut Vec<super::Settled>) {
        for (_, memo) in &mut self.entries {
            if let Some(completion) = memo.complete_cpu() {
                settled.push(completion);
            }
        }
    }
    pub(in crate::basis::nest) fn has_resources(&self) -> bool {
        self.entries.iter().any(|(_, memo)| memo.has_resources())
    }
    pub(in crate::basis::nest) fn take_metadata(&mut self) -> Option<Memo> {
        let at = self
            .entries
            .iter()
            .position(|(_, memo)| !memo.has_resources())?;
        Some(self.entries.remove(at).1)
    }
    /// Remove under the Span lock, release Page/freeze outside it after the fence.
    pub(in crate::basis::nest) fn take_deleted(
        &mut self,
        base: VirtAddr,
        active: &[ActiveRange],
    ) -> Option<Memo> {
        let at = self.entries.iter().position(|(page, memo)| {
            !memo.busy()
                && !active
                    .iter()
                    .any(|range| range.contains(base.get() + page * PAGE_SIZE))
        })?;
        Some(self.entries.remove(at).1)
    }
}
