use kernel_api::{
    abi::call::pend::PendFail,
    address::{PageCount, PageOffset},
    model::pend::{NestScope, PendRange, PendScope},
};

/// Public requests use parent-relative coordinates; retained source references
/// already carry absolute source coordinates. Both must stay within the parent.
#[derive(Clone, Copy)]
pub(crate) enum Window {
    Parent(PendRange),
    Source(PageOffset, PageCount),
}

impl Window {
    pub(crate) fn scope(
        self,
        parent: PendScope,
        requested: PendScope,
    ) -> Result<PendScope, PendFail> {
        if let Self::Parent(range) = self {
            if range != requested.range().unwrap_or(PendRange::Inherit) {
                return Err(PendFail::Denied);
            }
        }
        let range = match (parent, requested) {
            (PendScope::Object, PendScope::Object)
            | (PendScope::Nest(NestScope::Whole), PendScope::Nest(NestScope::Whole)) => {
                return if matches!(self, Self::Parent(PendRange::Inherit)) {
                    Ok(requested)
                } else {
                    Err(PendFail::Denied)
                };
            }
            (PendScope::Pages(range), PendScope::Pages(_)) => range,
            (
                PendScope::Nest(NestScope::Mapping { id, range }),
                PendScope::Nest(NestScope::Mapping { id: next, .. }),
            ) if id == next => range,
            _ => return Err(PendFail::Denied),
        };
        let PendRange::Pages { offset, count } = range else {
            return Err(PendFail::Denied);
        };
        let (offset, count) = self.resolve((
            PageOffset::new(offset),
            PageCount::new(count).ok_or(PendFail::Denied)?,
        ))?;
        let range = PendRange::Pages {
            offset: offset.get(),
            count: count.get(),
        };
        Ok(match requested {
            PendScope::Pages(_) => PendScope::Pages(range),
            PendScope::Nest(NestScope::Mapping { id, .. }) => {
                PendScope::Nest(NestScope::Mapping { id, range })
            }
            _ => return Err(PendFail::Denied),
        })
    }

    pub(crate) fn resolve(
        self,
        parent: (PageOffset, PageCount),
    ) -> Result<(PageOffset, PageCount), PendFail> {
        let (offset, count) = match self {
            Self::Parent(PendRange::Inherit) => (parent.0.get(), parent.1),
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
