//! Generation-qualified Cell operations. Creation, association, publication and
//! physical reshaping keep their own preparation and commit paths local.
mod association;
mod creation;
mod publication;
mod shape;
use association::associate;
pub(super) use creation::create;
use publication::publication;
use shape::reshape;

use super::*;
use crate::basis::cell::page::Page;
use crate::basis::nest::authority::external;
use kernel_api::model::cell::*;
fn reference(id: usize) -> SlotId {
    SlotId::from_bytes(&(id as u64).to_le_bytes()).unwrap()
}
fn position(at: KVirt, ident: &Arc<TaskIdent>) -> Result<CellPosition, CellFail> {
    CellPosition::from_bytes(&read_snapshot::<{ CellPosition::WIDTH }>(
        &ident.team.nest,
        at,
    )?)
    .ok_or(CellFail::NotAligned)
}
fn index(entry: &RangeEntry, page: usize) -> Result<NestRange, CellFail> {
    entry
        .mapping()
        .ok_or(CellFail::Denied)?
        .range(
            PageOffset::new(page),
            PageCount::new(entry.mapping().ok_or(CellFail::Denied)?.source().grain()).unwrap(),
        )
        .map_err(CellFail::from)
}
fn check(entry: &RangeEntry, actor: &Arc<Task>) -> Result<(), CellFail> {
    if !pend::live(actor) {
        return Err(CellFail::Denied);
    }
    let source = &entry.source;
    if let Some((meta, root, _)) = source.root() {
        let token = entry
            .control
            .as_ref()
            .and_then(|c| c.control().1)
            .unwrap_or(root);
        let authority = meta.authority(token).ok_or(CellFail::Denied)?;
        if !authority.associates() || !source.branch_check(Some(&authority), Access::READ) {
            return Err(CellFail::Denied);
        }
    } else if source.revoked() {
        return Err(CellFail::Denied);
    }
    Ok(())
}
use crate::basis::nest::authority::memo::Lease;
use kernel_api::model::pend::MemoRights;

fn with_memo<T>(
    pos: CellPosition,
    rights: MemoRights,
    ident: &Arc<TaskIdent>,
    run: impl FnOnce(&Lease, usize) -> Result<T, CellFail>,
) -> Result<T, CellFail> {
    let actor = caller(ident).ok_or(CellFail::Denied)?;
    let CellObject::Memo(token) = pos.object else {
        return Err(CellFail::Denied);
    };
    let lease = Lease::acquire(&actor, token, rights)?;
    let page = lease.page(pos.page)?;
    let result = run(&lease, page);
    if !lease.0.authority().alive() || !pend::live(&actor) {
        return Err(CellFail::Denied);
    }
    result
}
fn cpu<T>(
    lease: &Lease,
    run: impl FnOnce(&Arc<Nest>, &RangeEntry) -> Result<T, CellFail>,
) -> Result<T, CellFail> {
    let memo = lease.memo();
    let entry = memo
        .nest
        .mappings
        .locate(memo.mapping.get())
        .ok_or(CellFail::Denied)?;
    let span = entry.mapping().ok_or(CellFail::Denied)?;
    let _operation = span.operation().ok_or(CellFail::Busy)?;
    run(&memo.nest, &entry)
}
fn inspect(pos: CellPosition, ident: &Arc<TaskIdent>) -> Result<CellInfo, CellFail> {
    let actor = caller(ident).ok_or(CellFail::Denied)?;
    match pos.object {
        CellObject::Slot(r) => {
            let _serial = actor.pend.lock();
            let range = endpoint(&actor, r)?.cell(PageOffset::new(pos.page))?;
            let page = range
                .materialized_cells()?
                .into_iter()
                .next()
                .map(|(_, p)| p);
            Ok(CellInfo {
                level: range.level(),
                generation: 0,
                state: if page.is_some() {
                    CellState::Ready
                } else {
                    CellState::Vacant
                },
                page: page.as_ref().map_or(0, |p| p.id().get() as u64),
                frozen: page.as_ref().is_some_and(|p| p.frozen()),
                external: page
                    .as_ref()
                    .is_some_and(|p| p.exposure() == crate::basis::cell::page::Exposure::External),
                address: KVirt::new(0),
                fault_cause: 0,
                fault_address: 0,
            })
        }
        CellObject::Memo(token) => with_memo(pos, MemoRights::INSPECT, ident, |lease, page| {
            if lease.memo().nest.is_external() {
                if page != 0 {
                    return Err(CellFail::NoRegion);
                }
                external::snapshot(&actor, token, lease.memo().mapping)
            } else {
                cpu(lease, |nest, entry| {
                    let selected = index(entry, page)?;
                    let _ = nest.retry_invalidation();
                    nest.finish_memo(selected.mapping(), page);
                    Ok(selected.mapping().inspect_memo(page)?)
                })
            }
        }),
    }
}
pub(super) fn execute(call: CellCall, ident: &Arc<TaskIdent>) -> Result<usize, CellFail> {
    match call {
        CellCall::Create { .. } => unreachable!(),
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
            transfer(from, source.page, to, target.page, None, ident)
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
            with_memo(pos, MemoRights::REMOVE, ident, |lease, page| {
                if lease.memo().nest.is_external() {
                    if page != 0 {
                        return Err(CellFail::NoRegion);
                    }
                    let actor = caller(ident).ok_or(CellFail::Denied)?;
                    let _commit = crate::basis::commit();
                    external::remove(
                        &actor,
                        lease.0.token(),
                        lease.memo().mapping,
                        pos.generation,
                    )?;
                } else {
                    cpu(lease, |nest, entry| {
                        nest.retry_invalidation()?;
                        let selected = index(entry, page)?;
                        nest.finish_memo(selected.mapping(), page);
                        nest.remove_memo(selected.mapping(), page, pos.generation)?;
                        Ok(())
                    })?;
                }
                Ok(0)
            })
        }
        CellCall::Protect {
            position: at,
            access,
        } => {
            let pos = position(at, ident)?;
            let access = Access::from_wire(access)
                .filter(|a| a.is_leaf_set())
                .ok_or(CellFail::Denied)?;
            with_memo(pos, MemoRights::PROTECT, ident, |lease, page| {
                if lease.memo().nest.is_external() {
                    return Err(CellFail::Denied);
                }
                if access.contains(Access::WRITE) && lease.0.modifiable() != Modifiable::Yes {
                    return Err(CellFail::Denied);
                }
                cpu(lease, |nest, entry| {
                    nest.retry_invalidation()?;
                    let selected = index(entry, page)?;
                    nest.finish_memo(selected.mapping(), page);
                    selected.mapping().advance_memo(page, pos.generation)?;
                    nest.set_access(selected, access)?;
                    if access.contains(Access::EXECUTE) {
                        identity::sync_instructions()?;
                    }
                    Ok(0)
                })
            })
        }
        CellCall::Delete { object: at } => {
            let object = CellObject::from_bytes(&read_snapshot::<{ CellObject::WIDTH }>(
                &ident.team.nest,
                at,
            )?)
            .ok_or(CellFail::NotAligned)?;
            match object {
                CellObject::Slot(r) => release(r, ident),
                CellObject::Memo(_) => with_memo(
                    CellPosition {
                        object,
                        page: 0,
                        generation: 0,
                    },
                    MemoRights::DELETE,
                    ident,
                    |lease, _| {
                        let memo = lease.memo();
                        if lease.0.authority().window() != Some((PageOffset::ZERO, memo.count)) {
                            return Err(CellFail::Denied);
                        }
                        if memo.nest.is_external() {
                            let actor = caller(ident).ok_or(CellFail::Denied)?;
                            let _commit = crate::basis::commit();
                            external::delete(&actor, lease.0.token(), memo.mapping)?;
                        } else {
                            cpu(lease, |nest, entry| {
                                nest.retry_invalidation()?;
                                let span = entry.mapping().ok_or(CellFail::Denied)?;
                                if nest.has_published(span) {
                                    return Err(CellFail::Busy);
                                }
                                nest.detach(span.whole())?;
                                nest.retire_mapping(memo.mapping.get());
                                Ok(())
                            })?;
                        }
                        Ok(0)
                    },
                ),
            }
        }
    }
}
