use super::{MemoryError, Region};
use kernel_api::address::PAGE_SIZE;

/// Bounded, sorted, disjoint byte ranges. All arithmetic is checked at admission.
/// Capacity is an explicit early-boot limit, never permission to drop a range.
#[derive(Clone, Copy, Debug)]
pub(super) struct Ranges<const N: usize> {
    entries: [Region; N],
    len: usize,
}

impl<const N: usize> Ranges<N> {
    pub(super) const fn new() -> Self {
        Self {
            entries: [Region::new(0, 0); N],
            len: 0,
        }
    }

    pub(super) fn as_slice(&self) -> &[Region] {
        &self.entries[..self.len]
    }

    pub(super) fn insert(&mut self, region: Region, overlap: bool) -> Result<(), MemoryError> {
        let mut start = region.base;
        let mut end = checked_end(region)?;
        let mut first = 0;
        while first < self.len && self.entries[first].range().end < start {
            first += 1;
        }
        let mut last = first;
        while last < self.len && self.entries[last].base <= end {
            let old = self.entries[last];
            if !overlap && start < old.range().end && old.base < end {
                return Err(MemoryError::Overlap);
            }
            start = start.min(old.base);
            end = end.max(old.range().end);
            last += 1;
        }
        if first == last {
            if self.len == N {
                return Err(MemoryError::Capacity);
            }
            self.entries.copy_within(first..self.len, first + 1);
            self.len += 1;
        } else {
            self.entries.copy_within(last..self.len, first + 1);
            self.len -= last - first - 1;
        }
        self.entries[first] = Region::new(start, end - start);
        Ok(())
    }

    pub(super) fn contains(&self, region: Region) -> bool {
        let Ok(end) = checked_end(region) else {
            return false;
        };
        self.as_slice()
            .iter()
            .any(|r| r.base <= region.base && end <= r.range().end)
    }

    pub(super) fn overlaps(&self, region: Region) -> bool {
        self.as_slice()
            .iter()
            .any(|r| r.base < region.range().end && region.base < r.range().end)
    }

    pub(super) fn envelope(&self) -> Result<Region, MemoryError> {
        let first = self.as_slice().first().ok_or(MemoryError::Empty)?;
        let last = self.as_slice().last().ok_or(MemoryError::Empty)?;
        Ok(Region::new(first.base, last.range().end - first.base))
    }

    pub(super) fn subtract<const M: usize, const K: usize>(
        &self,
        reserved: &Ranges<M>,
    ) -> Result<Ranges<K>, MemoryError> {
        let mut free = Ranges::new();
        for ram in self.as_slice() {
            let end = ram.range().end;
            let mut start = ram.base;
            for hole in reserved.as_slice() {
                if hole.range().end <= start {
                    continue;
                }
                if hole.base >= end {
                    break;
                }
                free.insert_pages(start, hole.base.min(end))?;
                start = start.max(hole.range().end).min(end);
            }
            free.insert_pages(start, end)?;
        }
        Ok(free)
    }

    fn insert_pages(&mut self, start: usize, end: usize) -> Result<(), MemoryError> {
        let start = start
            .checked_next_multiple_of(PAGE_SIZE)
            .ok_or(MemoryError::Range)?;
        let end = end / PAGE_SIZE * PAGE_SIZE;
        if start < end {
            self.insert(Region::new(start, end - start), false)?;
        }
        Ok(())
    }
}

pub(super) fn checked_end(region: Region) -> Result<usize, MemoryError> {
    if region.size == 0 {
        return Err(MemoryError::Range);
    }
    region
        .base
        .checked_add(region.size)
        .ok_or(MemoryError::Range)
}

pub(super) fn covering_pages(region: Region) -> Result<Region, MemoryError> {
    let end = checked_end(region)?
        .checked_next_multiple_of(PAGE_SIZE)
        .ok_or(MemoryError::Range)?;
    let start = region.base / PAGE_SIZE * PAGE_SIZE;
    Ok(Region::new(start, end - start))
}
