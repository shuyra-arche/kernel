//! Generation-qualified Slot and scoped mapping operations.
mod association;
mod publication;
mod shape;
use association::associate;
use publication::publication;
use shape::reshape;

use super::*;
use crate::basis::cell::page::Page;
use crate::basis::pend::access::Mapping;
use kernel_api::model::cell::*;
use kernel_api::model::pend::NestRights;

fn position(at: KVirt, ident: &Arc<TaskIdent>) -> Result<CellPosition, CellFail> {
    CellPosition::from_bytes(&read_snapshot::<{ CellPosition::WIDTH }>(
        &ident.team.nest,
        at,
    )?)
    .ok_or(CellFail::NotAligned)
}

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

fn inspect(pos: CellPosition, ident: &Arc<TaskIdent>) -> Result<CellInfo, CellFail> {
    match pos.object {
        CellObject::Slot(slot) => {
            let actor = caller(ident).ok_or(CellFail::Denied)?;
            let _serial = actor.pend.lock();
            let range = endpoint(&actor, slot)?.cell(PageOffset::new(pos.page))?;
            let page = range
                .materialized_cells()?
                .into_iter()
                .next()
                .map(|(_, page)| page);
            Ok(CellInfo {
                level: range.level(),
                generation: 0,
                state: if page.is_some() {
                    CellState::Ready
                } else {
                    CellState::Vacant
                },
                page: page.as_ref().map_or(0, |page| page.id().get() as u64),
                frozen: page.as_ref().is_some_and(|page| page.frozen()),
                external: page.as_ref().is_some_and(|page| {
                    page.exposure() == crate::basis::cell::page::Exposure::External
                }),
                address: KVirt::new(0),
                fault_cause: 0,
                fault_address: 0,
            })
        }
        CellObject::Pend(_) => with_mapping(pos, NestRights::INSPECT, ident, |_, mapping, page| {
            let span = mapping.span().map_err(source_fail)?;
            mapping.nest().progress_mappings();
            let _ = mapping.nest().retry_invalidation();
            mapping.nest().finish_memo(&span, page);
            span.inspect_memo(page).map_err(CellFail::from)
        }),
    }
}

pub(super) fn execute(call: CellCall, ident: &Arc<TaskIdent>) -> Result<usize, CellFail> {
    match call {
        CellCall::Create {
            license,
            allocation,
        } => {
            let actor = caller(ident).ok_or(CellFail::Denied)?;
            let _serial = actor.pend.lock();
            let _commit = crate::basis::commit();
            resolve_source(
                &actor,
                SourceSpec::Allocation {
                    license,
                    allocation,
                },
            )
        }
        CellCall::Delete { slot } => release(slot, ident),
        CellCall::Split { slot } => reshape(slot, false, ident),
        CellCall::Merge { slot } => reshape(slot, true, ident),
        CellCall::Push { request } | CellCall::Pull { request } => {
            let spec = CellHandoff::from_bytes(&read_snapshot::<{ CellHandoff::WIDTH }>(
                &ident.team.nest,
                request,
            )?)
            .ok_or(CellFail::NotAligned)?;
            let (source, target) = if matches!(call, CellCall::Push { .. }) {
                (spec.local, spec.remote)
            } else {
                (spec.remote, spec.local)
            };
            let (CellObject::Slot(from), CellObject::Slot(to)) = (source.object, target.object)
            else {
                return Err(CellFail::Denied);
            };
            transfer(from, source.page, to, target.page, ident)
        }
        CellCall::Insert { request } => associate(
            CellAssociation::from_bytes(&read_snapshot::<{ CellAssociation::WIDTH }>(
                &ident.team.nest,
                request,
            )?)
            .ok_or(CellFail::NotAligned)?,
            ident,
        ),
        CellCall::Publish { position: at } => publication(position(at, ident)?, true, ident),
        CellCall::Retract { position: at } => publication(position(at, ident)?, false, ident),
        CellCall::Inspect { position: at, out } => {
            let info = inspect(position(at, ident)?, ident)?;
            if !ident.team.nest.copy_out(&info.to_bytes(), out) {
                return Err(CellFail::Denied);
            }
            Ok(0)
        }
        CellCall::Remove { position: at } => {
            let pos = position(at, ident)?;
            with_mapping(pos, NestRights::REMOVE, ident, |_, mapping, page| {
                let span = mapping.span().map_err(source_fail)?;
                mapping.nest().remove_memo(&span, page, pos.generation)?;
                Ok(0)
            })
        }
        CellCall::Protect {
            position: at,
            access,
        } => {
            let pos = position(at, ident)?;
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
    }
}
