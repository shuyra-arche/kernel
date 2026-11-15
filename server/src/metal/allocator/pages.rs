//! Compact indices over admitted free-RAM regions. Holes consume no entries.
//! This immutable geometry is shared by Frame, Tally and diagnostics; it is
//! never a free-page ledger or an ownership/physical-access capability.
use kernel_api::address::PAGE_SIZE;
use kernel_api::boot::platform::Region;

#[derive(Clone, Copy)]
pub(crate) struct Pages<'a> {
    regions: &'a [Region],
}

impl<'a> Pages<'a> {
    pub(crate) fn new(regions: &'a [Region]) -> Self {
        Self { regions }
    }

    pub(crate) fn len(self) -> usize {
        self.regions
            .iter()
            .map(|region| region.size / PAGE_SIZE)
            .sum()
    }

    pub(crate) fn index(self, address: usize) -> Option<usize> {
        let mut offset = 0;
        for region in self.regions {
            if region.range().contains(&address) {
                return Some(offset + (address - region.base) / PAGE_SIZE);
            }
            offset += region.size / PAGE_SIZE;
        }
        None
    }

    pub(crate) fn address(self, mut index: usize) -> Option<usize> {
        for region in self.regions {
            let pages = region.size / PAGE_SIZE;
            if index < pages {
                return Some(region.base + index * PAGE_SIZE);
            }
            index -= pages;
        }
        None
    }
}
