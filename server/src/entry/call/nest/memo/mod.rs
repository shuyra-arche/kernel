//! Mapping-scoped operations; the v10 adapter supplies normalized descriptors.
use super::super::memory::{caller, source_fail};
use crate::basis::{
    cell::page::Page,
    nest::Access,
    pend::{self, access::Mapping},
    unit::task::{Task, TaskIdent},
};
use alloc::sync::Arc;
use kernel_api::{
    abi::call::cell::CellFail,
    address::{PageCount, PageOffset},
    model::{cell::*, modifiable::Modifiable, pend::NestRights},
};
mod association;
mod publication;
pub(in crate::entry::call) use association::associate;
pub(in crate::entry::call) use publication::publication;
fn with_mapping<T>(
    pos: CellPosition,
    rights: NestRights,
    ident: &Arc<TaskIdent>,
    run: impl FnOnce(&Arc<Task>, &Mapping, usize) -> Result<T, CellFail>,
) -> Result<T, CellFail> {
    let actor = caller(ident).ok_or(CellFail::Denied)?;
    let _serial = actor.pend.lock();
    let CellObject::Pend(token) = pos.object else {
        return Err(CellFail::Denied);
    };
    let mapping = Mapping::acquire(&actor, token, rights).map_err(source_fail)?;
    let page = mapping.page(pos.page).map_err(source_fail)?;
    let result = run(&actor, &mapping, page);
    if !pend::live(&actor) || !mapping.entry().authority().alive() {
        return Err(CellFail::Denied);
    }
    result
}

pub(in crate::entry::call) fn inspect(
    pos: CellPosition,
    ident: &Arc<TaskIdent>,
) -> Result<CellInfo, CellFail> {
    with_mapping(pos, NestRights::INSPECT, ident, |_, mapping, page| {
        let span = mapping.span().map_err(source_fail)?;
        mapping.nest().progress_mappings();
        let _ = mapping.nest().retry_invalidation();
        mapping.nest().finish_memo(&span, page);
        span.inspect_memo(page).map_err(CellFail::from)
    })
}
pub(in crate::entry::call) fn remove(
    pos: CellPosition,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    with_mapping(pos, NestRights::REMOVE, ident, |_, mapping, page| {
        let span = mapping.span().map_err(source_fail)?;
        mapping.nest().remove_memo(&span, page, pos.generation)?;
        Ok(0)
    })
}
pub(in crate::entry::call) fn protect(
    pos: CellPosition,
    access: u64,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    let access = Access::from_wire(access)
        .filter(|access| access.is_leaf_set())
        .ok_or(CellFail::Denied)?;
    with_mapping(pos, NestRights::PROTECT, ident, |_, mapping, page| {
        let span = mapping.span().map_err(source_fail)?;
        mapping.nest().protect_memo(
            &span,
            page,
            pos.generation,
            access,
            mapping.entry().authority(),
        )?;
        Ok(0)
    })
}
