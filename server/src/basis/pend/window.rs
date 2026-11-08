use kernel_api::{
    abi::call::pend::PendFail,
    address::{PageCount, PageOffset},
    model::pend::PendRange,
};

/// Public requests use parent-relative coordinates; retained source references
/// already carry absolute source coordinates. Both must stay within the parent.
#[derive(Clone, Copy)]
pub(crate) enum Window {
    Parent(PendRange),
    Source(PageOffset, PageCount),
}

impl Window {
    pub(crate) fn resolve(
        self,
        parent: (PageOffset, PageCount),
    ) -> Result<(PageOffset, PageCount), PendFail> {
        let (offset, count) = match self {
            Self::Parent(PendRange::Inherit) => return Ok(parent),
            Self::Parent(PendRange::Pages { offset, count }) => (
                parent.0.get().checked_add(offset).ok_or(PendFail::Denied)?,
                PageCount::new(count).ok_or(PendFail::Denied)?,
            ),
            Self::Source(offset, count) => (offset.get(), count),
        };
        let parent_end = parent
            .0
            .get()
            .checked_add(parent.1.get())
            .ok_or(PendFail::Denied)?;
        let end = offset.checked_add(count.get()).ok_or(PendFail::Denied)?;
        if offset < parent.0.get()
            || end > parent_end
            || end
                .checked_mul(kernel_api::model::cell::PAGE_SIZE)
                .is_none()
        {
            return Err(PendFail::Denied);
        }
        Ok((PageOffset::new(offset), count))
    }
}
