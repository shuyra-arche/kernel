//! Withdraw source-qualified uses without destroying independent reservations.
use crate::basis::{
    flat::sharing::Sharing,
    pend::{RangeBlock, SourceAuthorization},
};
use alloc::sync::Arc;
use kernel_api::{abi::call::pend::PendFail, model::identity::PendId};

pub(crate) fn withdraw_source_branch(
    meta: &Arc<Sharing>,
    token: PendId,
    clear: bool,
) -> Result<(), PendFail> {
    let Some(source) = meta.authorization() else {
        return Ok(());
    };
    if let Some(branch) = meta.authority(token) {
        source.retire_origins(Some(&branch), None)?;
    }
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
    source.retire_origins(None, None)?;
    for binding in source.bindings()? {
        let nest = binding.nest().upgrade().ok_or(PendFail::Busy)?;
        nest.withdraw_source(&binding, source, None, None, clear)?;
    }
    Ok(())
}

pub(crate) fn withdraw_source_window(
    source: &Arc<SourceAuthorization>,
    block: &RangeBlock,
) -> Result<(), PendFail> {
    source.retire_origins(None, Some(block))?;
    let (offset, count) = block.window();
    let end = offset
        .get()
        .checked_add(count.get())
        .ok_or(PendFail::NotAligned)?;
    for binding in source.bindings()? {
        let Some(page) = binding.page() else {
            continue;
        };
        let nest = {
            let _commit = crate::basis::commit();
            let nest = binding.nest().upgrade().ok_or(PendFail::Busy)?;
            let Some(selected) = nest.binding_source(&binding) else {
                continue;
            };
            let (base, size) = selected.window();
            let limit = base
                .get()
                .checked_add(size.get())
                .ok_or(PendFail::NotAligned)?;
            if !Arc::ptr_eq(selected.source(), source) || base.get() >= end || limit <= offset.get()
            {
                continue;
            }
            let entry = nest
                .reservations
                .registered(binding.id())
                .ok_or(PendFail::Busy)?;
            let span = entry.reservation().ok_or(PendFail::Busy)?;
            let Some(data) = nest.memo_page(span, page).map_err(|_| PendFail::Busy)? else {
                continue;
            };
            block.retain(data)?;
            nest
        };
        nest.withdraw_source(&binding, source, None, None, false)?;
    }
    Ok(())
}
