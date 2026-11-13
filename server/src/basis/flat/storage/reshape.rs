//! The Flat's holding, geometry, versions and physical maintenance gates change
//! together. Receipt conversion is the last fallible operation.
use super::{FlatSource, FlatState, Slot, residency::Error};
use crate::basis::cell::{
    change,
    page::{Block, PageError},
};
use alloc::{sync::Arc, vec::Vec};
use kernel_api::{
    address::{PageCount, PageIndex},
    model::{
        cell::CellLevel,
        flat::{FlatVersion, Reshape, SlotVersion},
    },
};

#[derive(Clone, Copy)]
pub(crate) enum Direction {
    Split,
    Combine,
}
impl Direction {
    pub(crate) fn levels(self, range: Reshape) -> (CellLevel, CellLevel) {
        match self {
            Self::Split => (range.parent(), range.child()),
            Self::Combine => (range.child(), range.parent()),
        }
    }
}
/// Indices are valid only while the same Flat lock is held. Planning performs no
/// allocation or holding movement; preparation reserves all commit capacity.
struct Plan {
    start: usize,
    end: usize,
    old_grain: usize,
    new: CellLevel,
    new_grain: usize,
    count: usize,
    first: usize,
    last: usize,
    origin: Option<usize>,
}
impl FlatState {
    fn plan(
        &self,
        initial: CellLevel,
        range: Reshape,
        direction: Direction,
    ) -> Result<Plan, Error> {
        self.explicit()?;
        let start = range.offset();
        let end = start + range.count();
        let (old, new) = direction.levels(range);
        let old_grain = old.pages().unwrap();
        let new_grain = new.pages().unwrap();
        let count = range.count() / new_grain;
        if self.is_pinned(
            PageIndex::new(start),
            PageCount::new(range.count()).unwrap(),
        ) || self
            .staged
            .iter()
            .any(|(at, _)| (start..end).contains(&at.get()))
        {
            return Err(Error::Busy);
        }
        for head in (start..end).step_by(old_grain) {
            if self.geometry.head(head, initial) != (head, old) {
                return Err(Error::Unsupported);
            }
        }
        // One parent cannot discard distinct original branch restrictions.
        // Splitting carries the same restriction to every resulting child.
        let origin = self.provenance.iter().position(|(at, _)| at.get() == start);
        for head in (start..end).step_by(old_grain) {
            let other = self
                .provenance
                .iter()
                .find(|(at, _)| at.get() == head)
                .map(|(_, p)| p);
            if match (origin.map(|at| &self.provenance[at].1), other) {
                (None, None) => false,
                (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
                _ => true,
            } {
                return Err(Error::Busy);
            }
        }
        let FlatSource::Managed(slots) = &self.source else {
            unreachable!()
        };
        let first = slots.partition_point(|(at, _)| at.get() < start);
        let last = slots.partition_point(|(at, _)| at.get() < end);
        if last - first != range.count() / old_grain {
            return Err(Error::Empty);
        }
        for (n, (at, slot)) in slots[first..last].iter().enumerate() {
            if at.get() != start + n * old_grain {
                return Err(Error::Empty);
            }
            match slot {
                Slot::Resident(page) if page.level() == old => {}
                Slot::Missing => return Err(Error::Empty),
                _ => return Err(Error::Busy),
            }
        }
        Ok(Plan {
            start,
            end,
            old_grain,
            new,
            new_grain,
            count,
            first,
            last,
            origin,
        })
    }
    pub fn reshape(
        &mut self,
        initial: CellLevel,
        range: Reshape,
        direction: Direction,
        expected: FlatVersion,
        blocks: &mut Vec<Block>,
    ) -> Result<(FlatVersion, SlotVersion), Error> {
        // No lock is released between the immutable plan and its application.
        // Check the version first so a stale retry never acts on new geometry.
        if self.revision.current() != expected {
            return Err(Error::Stale);
        }
        let Plan {
            start,
            end,
            old_grain,
            new,
            new_grain,
            count,
            first,
            last,
            origin,
        } = self.plan(initial, range, direction)?;
        self.revision.prepare_geometry(expected, count)?;
        self.geometry.prepare(count)?;
        self.provenance
            .try_reserve(count)
            .map_err(|_| Error::OutOfMemory)?;
        let origin = origin.map(|at| self.provenance[at].1.clone());
        let FlatSource::Managed(slots) = &mut self.source else {
            unreachable!()
        };
        slots.try_reserve(count).map_err(|_| Error::OutOfMemory)?;
        let mut pages = Vec::new();
        pages
            .try_reserve(last - first)
            .map_err(|_| Error::OutOfMemory)?;
        for (_, slot) in slots.drain(first..last) {
            let Slot::Resident(page) = slot else {
                unreachable!()
            };
            pages.push(page);
        }
        let result = match change::reshape(&mut pages, blocks, new) {
            Ok(result) => result,
            Err(error) => {
                for (n, page) in pages.into_iter().enumerate() {
                    slots.insert(
                        first + n,
                        (PageIndex::new(start + n * old_grain), Slot::Resident(page)),
                    );
                }
                return Err(match error {
                    PageError::OutOfMemory | PageError::IdentityExhausted => Error::OutOfMemory,
                    PageError::InvalidRange => Error::Unsupported,
                    _ => Error::Busy,
                });
            }
        };
        for (n, page) in result.into_iter().enumerate() {
            slots.insert(
                first + n,
                (PageIndex::new(start + n * new_grain), Slot::Resident(page)),
            );
        }
        self.geometry.replace(start, range.count(), new);
        self.provenance
            .retain(|(at, _)| !(start..end).contains(&at.get()));
        if let Some(origin) = origin {
            for head in (start..end).step_by(new_grain) {
                self.provenance.push((PageIndex::new(head), origin.clone()));
            }
        }
        Ok(self
            .revision
            .commit_geometry((start..end).step_by(new_grain).map(PageIndex::new)))
    }
}
