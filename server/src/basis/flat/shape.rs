//! Slot receipt reshaping under the pair of source locks.
use super::{Flat, FlatError, FlatSource, Slot};
use crate::basis::cell::page::{Page, PageError};
use alloc::vec::Vec;
use kernel_api::address::PageIndex;
impl Flat {
    pub(crate) fn reshape_into(&self, target: &Self) -> Result<(), FlatError> {
        if core::ptr::eq(self, target) || self.page_count != target.page_count {
            return Err(FlatError::OutOfRange);
        }
        crate::metal::sync::with_pair(&self.state, &target.state, |from, to| {
            if from.explicit
                || to.explicit
                || !from.transferable
                || !from.residencies.is_empty()
                || !to.residencies.is_empty()
            {
                return Err(FlatError::Busy);
            }
            let (FlatSource::Managed(slots), FlatSource::Managed(output)) =
                (&mut from.source, &mut to.source)
            else {
                return Err(FlatError::OutOfRange);
            };
            if !output.is_empty()
                || slots.len() != self.page_count.get() / self.grain()
                || slots.iter().enumerate().any(|(n, (index, slot))| {
                    index.get() != n * self.grain() || !matches!(slot, Slot::Resident(_))
                })
            {
                return Err(FlatError::Missing);
            }
            let mut pages = Vec::new();
            pages
                .try_reserve_exact(slots.len())
                .map_err(|_| FlatError::OutOfMemory)?;
            output
                .try_reserve_exact(target.page_count.get() / target.grain())
                .map_err(|_| FlatError::OutOfMemory)?;
            for (_, slot) in slots.drain(..) {
                let Slot::Resident(page) = slot else {
                    unreachable!()
                };
                pages.push(page);
            }
            match Page::reshape(&mut pages, target.level) {
                Ok(result) => {
                    for (n, page) in result.into_iter().enumerate() {
                        output.push((PageIndex::new(n * target.grain()), Slot::Resident(page)));
                    }
                    from.transferable = false;
                    Ok(())
                }
                Err(error) => {
                    for (n, page) in pages.into_iter().enumerate() {
                        slots.push((PageIndex::new(n * self.grain()), Slot::Resident(page)));
                    }
                    Err(match error {
                        PageError::Busy => FlatError::Busy,
                        PageError::OutOfMemory | PageError::IdentityExhausted => {
                            FlatError::OutOfMemory
                        }
                        _ => FlatError::NotAligned,
                    })
                }
            }
        })
    }
}
