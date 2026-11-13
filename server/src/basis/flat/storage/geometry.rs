//! Sparse overrides of the original allocation geometry. Coordinates never move.
use alloc::vec::Vec;
use kernel_api::{address::PageIndex, model::cell::CellLevel};
pub(in super::super) struct Geometry(Vec<(PageIndex, CellLevel)>);
impl Geometry {
    pub fn new() -> Self {
        Self(Vec::new())
    }
    pub fn head(&self, page: usize, initial: CellLevel) -> (usize, CellLevel) {
        let at = self.0.partition_point(|(index, _)| index.get() <= page);
        if let Some((index, level)) = at.checked_sub(1).map(|at| self.0[at]) {
            if page - index.get() < level.pages().unwrap() {
                return (index.get(), level);
            }
        }
        (
            page / initial.pages().unwrap() * initial.pages().unwrap(),
            initial,
        )
    }
    pub fn covers(&self, start: usize, count: usize, initial: CellLevel) -> bool {
        let Some(end) = start.checked_add(count).filter(|end| *end > start) else {
            return false;
        };
        let (last, level) = self.head(end - 1, initial);
        self.head(start, initial).0 == start && last + level.pages().unwrap() == end
    }
    pub fn prepare(&mut self, count: usize) -> Result<(), super::residency::Error> {
        self.0
            .try_reserve(count)
            .map_err(|_| super::residency::Error::OutOfMemory)
    }
    pub fn replace(&mut self, start: usize, count: usize, level: CellLevel) {
        self.0
            .retain(|(at, _)| at.get() < start || at.get() >= start + count);
        let mut at = self.0.partition_point(|(index, _)| index.get() < start);
        for index in (start..start + count).step_by(level.pages().unwrap()) {
            self.0.insert(at, (PageIndex::new(index), level));
            at += 1;
        }
    }
}
