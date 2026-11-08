use super::super::Error;
use kernel_api::address::PAGE_SIZE;

/// Original geometry names a boot resource; page coverage defines its authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Region {
    pub base: usize,
    pub size: usize,
    start: usize,
    end: usize,
}

impl Region {
    pub fn new(base: usize, size: usize) -> Result<Self, Error> {
        let end = base
            .checked_add(size)
            .filter(|_| size != 0)
            .and_then(|end| end.checked_next_multiple_of(PAGE_SIZE))
            .ok_or(Error::Description)?;
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
