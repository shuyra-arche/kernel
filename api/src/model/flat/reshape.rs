//! One parent-sized range and its immediate radix-512 children. This is checked
//! logical geometry, not an allocation receipt or permission to reshape Pages.

use crate::model::cell::CellLevel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reshape {
    offset: usize,
    parent: CellLevel,
}

impl Reshape {
    pub const CHILDREN: usize = 512;

    pub fn new(offset: usize, parent: CellLevel) -> Option<Self> {
        parent.child()?;
        parent
            .fits_at(offset, offset, parent.pages()?)
            .then_some(Self { offset, parent })
    }

    pub const fn offset(self) -> usize {
        self.offset
    }

    pub const fn parent(self) -> CellLevel {
        self.parent
    }

    pub fn child(self) -> CellLevel {
        self.parent.child().expect("reshape parent has children")
    }

    pub fn count(self) -> usize {
        self.parent.pages().expect("checked reshape extent")
    }

    /// A fixed-origin child head; never a coordinate relative to a narrowed grant.
    pub fn child_offset(self, index: usize) -> Option<usize> {
        if index >= Self::CHILDREN {
            return None;
        }
        self.offset
            .checked_add(index.checked_mul(self.child().pages()?)?)
    }

    /// The entire parent range must fit the live scope. The provider must obtain
    /// that scope and recheck its authority at the same commit boundary.
    pub fn fits(self, offset: usize, count: usize) -> bool {
        self.parent.fits_at(self.offset, offset, count)
    }
}
