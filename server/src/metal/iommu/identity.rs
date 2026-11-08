use core::num::NonZeroU32;
use core::num::NonZeroUsize;

use super::Error;
use crate::metal::generic::iommu::{BatchId, BindingId};

static NEXT_BINDING: crate::basis::name::Serial = crate::basis::name::Serial::new(1, usize::MAX);
static NEXT_BATCH: crate::basis::name::Serial =
    crate::basis::name::Serial::new(1, u32::MAX as usize);

impl BindingId {
    pub(super) fn allocate() -> Result<Self, Error> {
        let value = NEXT_BINDING
            .allocate()
            .and_then(NonZeroUsize::new)
            .ok_or(Error::Exhausted)?;
        // SAFETY: the boot-wide serial allocator never reuses a value.
        Ok(unsafe { Self::from_unique(value) })
    }
}

impl BatchId {
    pub(crate) fn allocate() -> Result<Self, Error> {
        let value = NEXT_BATCH
            .allocate()
            .and_then(|n| NonZeroU32::new(n as u32))
            .ok_or(Error::Exhausted)?;
        // SAFETY: the boot-wide serial allocator never reuses a value.
        Ok(unsafe { Self::from_unique(value) })
    }
}
