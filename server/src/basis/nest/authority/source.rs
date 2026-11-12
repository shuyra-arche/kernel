//! Withdraw source-qualified uses without destroying independent reservations.
use alloc::sync::Arc;
use kernel_api::{
    abi::call::pend::PendFail,
    address::{PageCount, PageOffset},
    model::identity::PendId,
};

use crate::basis::{
    flat::sharing::Sharing,
    pend::{Pend, SourceAuthorization},
};

pub(crate) fn withdraw_source_branch(
    meta: &Arc<Sharing>,
    token: PendId,
    clear: bool,
) -> Result<(), PendFail> {
    let Some(source) = meta.authorization() else {
        return Ok(());
    };
    for binding in source.bindings()? {
        if binding.page().is_none() {
            continue;
        }
        let nest = binding.nest().upgrade().ok_or(PendFail::Busy)?;
        nest.withdraw_source(&binding, &source, Some(token), None, clear)?;
    }
    Ok(())
}

pub(crate) fn withdraw_source(
    source: &Arc<SourceAuthorization>,
    clear: bool,
) -> Result<(), PendFail> {
    for binding in source.bindings()? {
        let nest = binding.nest().upgrade().ok_or(PendFail::Busy)?;
        nest.withdraw_source(&binding, source, None, None, clear)?;
    }
    Ok(())
}

pub(crate) fn withdraw_source_window(
    source: &Arc<SourceAuthorization>,
    (offset, count): (PageOffset, PageCount),
    except: Option<&Arc<Pend>>,
) -> Result<(), PendFail> {
    let end = offset
        .get()
        .checked_add(count.get())
        .ok_or(PendFail::NotAligned)?;
    for binding in source.bindings()? {
        if binding.page().is_none() {
            continue;
        }
        let nest = binding.nest().upgrade().ok_or(PendFail::Busy)?;
        let Some(selected) = nest.binding_source(&binding) else {
            continue;
        };
        let (base, size) = selected.window();
        let limit = base
            .get()
            .checked_add(size.get())
            .ok_or(PendFail::NotAligned)?;
        if base.get() >= end || limit <= offset.get() {
            continue;
        }
        let parent = selected
            .hold()
            .authority()
            .cloned()
            .or_else(|| selected.source().root().map(|(_, _, authority)| authority));
        if except.is_some_and(|except| {
            parent
                .as_ref()
                .is_some_and(|parent| Arc::ptr_eq(parent, except))
        }) {
            continue;
        }
        nest.withdraw_source(&binding, source, None, None, false)?;
    }
    Ok(())
}
