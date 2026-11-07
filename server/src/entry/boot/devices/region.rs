use crate::core::memory::PAGE_SIZE;
use kernel_api::abi::call::pie::PieFail;

/// Original geometry names a boot resource; page coverage defines its authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Region {
    pub base: usize,
    pub size: usize,
    start: usize,
    end: usize,
}

impl Region {
    pub fn new(base: usize, size: usize) -> Result<Self, PieFail> {
        let end = base
            .checked_add(size)
            .filter(|_| size != 0)
            .and_then(|end| end.checked_next_multiple_of(PAGE_SIZE))
            .ok_or(PieFail::Denied)?;
        Ok(Self {
            base,
            size,
            start: base & !(PAGE_SIZE - 1),
            end,
        })
    }

    pub fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}
