//! Physical Cell geometry. Coordinates remain in ABI base pages.
use crate::address::BASE_PAGE_SIZE;
/// A radix-512 level in the RV64 contract. Admission checks the selected Sv mode
/// separately; representable geometry does not promise available physical RAM.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct CellLevel(u8);
impl CellLevel {
    pub const BASE: Self = Self(0);
    pub fn new(level: usize) -> Option<Self> {
        let value = Self(u8::try_from(level).ok()?);
        value.bytes().filter(|size| *size <= isize::MAX as usize)?;
        Some(value)
    }
    pub const fn get(self) -> usize {
        self.0 as usize
    }
    pub fn pages(self) -> Option<usize> {
        1usize.checked_shl(u32::from(self.0) * 9)
    }
    pub fn bytes(self) -> Option<usize> {
        self.pages()?.checked_mul(BASE_PAGE_SIZE)
    }
    pub fn child(self) -> Option<Self> {
        self.0.checked_sub(1).map(Self)
    }
    pub fn parent(self) -> Option<Self> {
        Self::new(self.get() + 1)
    }

    /// One complete physical unit at a fixed-origin coordinate inside a window.
    /// Pure geometry; callers must separately validate current identity/authority.
    pub(crate) fn fits_at(self, page: usize, offset: usize, count: usize) -> bool {
        let Some(grain) = self.pages() else {
            return false;
        };
        super::pages(offset, count).is_some()
            && page.is_multiple_of(grain)
            && page >= offset
            && page
                .checked_add(grain)
                .is_some_and(|end| end <= offset + count)
    }
}
