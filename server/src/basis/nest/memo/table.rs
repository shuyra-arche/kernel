//! Sparse positions keyed by the reservation identity and page coordinate.
//! Address reuse cannot recover the former reservation's Memo.
use super::{Memo, Publication};
use crate::basis::nest::{NestError, Span};
use alloc::vec::Vec;

pub(in crate::basis::nest) struct Memos {
    entries: Vec<((usize, usize), Memo)>,
}
impl Memos {
    pub(in crate::basis::nest) fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
    pub(in crate::basis::nest) fn idle(&self) -> bool {
        self.entries
            .iter()
            .all(|(_, memo)| memo.publication.is_none())
    }
    fn key(span: &Span, page: usize) -> (usize, usize) {
        (span.identity(), page)
    }
    pub(in crate::basis::nest) fn reserve(&mut self, additional: usize) -> Result<(), NestError> {
        self.entries
            .try_reserve(additional)
            .map_err(|_| NestError::OutOfMemory)
    }
    pub(in crate::basis::nest) fn get(&self, span: &Span, page: usize) -> Option<&Memo> {
        let key = Self::key(span, page);
        self.entries
            .binary_search_by_key(&key, |(k, _)| *k)
            .ok()
            .map(|i| &self.entries[i].1)
    }
    pub(in crate::basis::nest) fn get_mut(
        &mut self,
        span: &Span,
        page: usize,
    ) -> Result<&mut Memo, NestError> {
        let key = Self::key(span, page);
        let at = match self.entries.binary_search_by_key(&key, |(k, _)| *k) {
            Ok(at) => at,
            Err(at) => {
                self.reserve(1)?;
                self.entries.insert(at, (key, Memo::vacant()));
                at
            }
        };
        Ok(&mut self.entries[at].1)
    }
    pub(in crate::basis::nest) fn controlled(&self, span: &Span, page: usize) -> bool {
        self.get(span, page).is_some_and(|m| m.controlled)
    }
    pub(in crate::basis::nest) fn publication(
        &self,
        span: &Span,
        page: usize,
    ) -> Option<&Publication> {
        self.get(span, page).and_then(|m| m.publication.as_ref())
    }
    pub(in crate::basis::nest) fn published<'a>(
        &'a self,
        span: &Span,
    ) -> impl Iterator<Item = (usize, &'a Publication)> {
        let id = span.identity();
        self.entries
            .iter()
            .filter(move |((owner, _), _)| *owner == id)
            .filter_map(|((_, page), memo)| memo.publication.as_ref().map(|p| (*page, p)))
    }
    pub(in crate::basis::nest) fn publish(
        &mut self,
        span: &Span,
        page: usize,
        publication: Publication,
    ) {
        let key = Self::key(span, page);
        let at = match self.entries.binary_search_by_key(&key, |(k, _)| *k) {
            Ok(at) => at,
            Err(at) => {
                assert!(
                    self.entries.len() < self.entries.capacity(),
                    "prepare Memo before hardware publication"
                );
                self.entries.insert(at, (key, Memo::vacant()));
                at
            }
        };
        let memo = &mut self.entries[at].1;
        assert!(memo.publication.is_none());
        if let Some(page) = &memo.page {
            assert!(alloc::sync::Arc::ptr_eq(page, publication.cell()));
        }
        memo.publication = Some(publication);
    }
    pub(in crate::basis::nest) fn take_publication(
        &mut self,
        span: &Span,
        page: usize,
    ) -> Option<Publication> {
        let key = Self::key(span, page);
        let at = self.entries.binary_search_by_key(&key, |(k, _)| *k).ok()?;
        self.entries[at].1.publication.take()
    }
    pub(in crate::basis::nest) fn withdraw(
        &mut self,
        span: &Span,
        first: usize,
        last: usize,
        retired: &mut Vec<Publication>,
    ) {
        for ((id, page), memo) in &mut self.entries {
            if *id == span.identity() && *page >= first && *page <= last {
                if let Some(publication) = memo.publication.take() {
                    retired.push(publication.retire());
                }
            }
        }
    }
    pub(in crate::basis::nest) fn complete(&mut self, span: &Span) {
        for ((id, _), memo) in &mut self.entries {
            if *id == span.identity() {
                if let Some(publication) = &mut memo.publication {
                    publication.complete();
                }
            }
        }
    }
    /// Remove under the Nest lock, release Page/freeze outside it after the fence.
    pub(in crate::basis::nest) fn take_deleted(&mut self, span: &Span) -> Option<Memo> {
        let active = span.state.lock();
        let at = self.entries.iter().position(|((id, page), _)| {
            *id == span.identity()
                && !active.active.iter().any(|r| {
                    r.contains(span.initial_start().get() + page * kernel_api::address::PAGE_SIZE)
                })
        })?;
        Some(self.entries.remove(at).1)
    }
}
