use crate::basis::nest::NestError;
use kernel_api::address::{PAGE_SIZE, PageCount, PageOffset, VirtAddr};
use kernel_api::model::cell::CellLevel;

/// The original geometry of one occupied span. Active access and the source
/// selection are separate state; a retired hole in the original range never
/// becomes accessible again through this geometry.
pub(crate) struct SpanRange {
    start: VirtAddr,
    size: usize,
    level: CellLevel,
}

impl SpanRange {
    pub(crate) fn new(start: VirtAddr, size: usize, level: CellLevel) -> Result<Self, NestError> {
        let cell = level.bytes().ok_or(NestError::NotAligned)?;
        if size == 0
            || start.offset() != 0
            || !size.is_multiple_of(cell)
            || start.as_usize().checked_add(size - 1).is_none()
        {
            return Err(NestError::NotAligned);
        }
        Ok(Self { start, size, level })
    }

    pub(crate) fn start(&self) -> VirtAddr {
        self.start
    }

    pub(crate) fn size(&self) -> usize {
        self.size
    }

    pub(crate) fn level(&self) -> CellLevel {
        self.level
    }

    /// Inclusive final byte of the original range.
    pub(crate) fn last(&self) -> usize {
        self.start.as_usize() + (self.size - 1)
    }

    /// Release complete Cells at coordinates fixed to the original range.
    /// Physical Cell geometry is distinct from the hardware PTE leaf size.
    pub(crate) fn release_slice(
        &self,
        offset: PageOffset,
        count: PageCount,
    ) -> Result<(usize, usize), NestError> {
        let grain = self.level.pages().ok_or(NestError::NotAligned)?;
        if !offset.get().is_multiple_of(grain) || !count.get().is_multiple_of(grain) {
            return Err(NestError::NotAligned);
        }
        let end = offset
            .get()
            .checked_add(count.get())
            .ok_or(NestError::NotAligned)?;
        if end > self.size / PAGE_SIZE {
            return Err(NestError::NotAligned);
        }
        let start = self
            .start
            .as_usize()
            .checked_add(
                offset
                    .get()
                    .checked_mul(PAGE_SIZE)
                    .ok_or(NestError::NotAligned)?,
            )
            .ok_or(NestError::NotAligned)?;
        let last = start
            .checked_add(count.checked_bytes().ok_or(NestError::NotAligned)? - 1)
            .ok_or(NestError::NotAligned)?;
        Ok((start, last))
    }
}
