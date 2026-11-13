//! Native range maintenance: close admission before draining, retain exact
//! Page gates through staging, and settle old uses before changing the owner.
use super::*;
use kernel_api::{
    address::{PageCount, PageOffset},
    model::{
        flat::BlockPhase,
        pend::{Operations, PendRange, PendScope},
    },
};

pub(in crate::entry::call) fn effect(
    actor: &Arc<Task>,
    action: Action,
    receipt: &mut crate::entry::call::next_memory::Receipt<'_>,
) -> Result<Value, Failure> {
    match action {
        Action::Block {
            start,
            count,
            recover,
        } => {
            let (entry, meta, source, block, _operation) = {
                let _serial = actor.pend.lock();
                let _commit = basis::commit();
                let range =
                    flat::authority::slot(actor, start, FlatRights::BLOCK).map_err(pend_error)?;
                if !range.flat().is_explicit() {
                    return Err(Failure::Unsupported);
                }
                let entry = pend::locate(actor, start.token()).ok_or(Failure::Denied)?;
                let meta = entry.target().memory().ok_or(Failure::Denied)?;
                let offset = start
                    .page()
                    .checked_sub(meta.source().index().get())
                    .ok_or(Failure::Denied)?;
                if !entry.authority().selects(
                    Operations::Flat(FlatRights::BLOCK),
                    PendScope::Pages(PendRange::Pages { offset, count }),
                ) {
                    return Err(Failure::Denied);
                }
                let count = PageCount::new(count).ok_or(Failure::NotAligned)?;
                meta.source()
                    .range(PageOffset::new(offset), count)
                    .map_err(|_| Failure::NotAligned)?;
                let source = meta.authorization().ok_or(Failure::Denied)?;
                let operation =
                    flat::sharing::SharingOperation::acquire(&meta).ok_or(Failure::Busy)?;
                let block = if let Some(reference) = recover {
                    let block = source.block_ref(reference).ok_or(Failure::Stale)?;
                    if block.owner_live()
                        || !block.owner().descends_from(entry.authority())
                        || block.window() != (PageOffset::new(offset), count)
                    {
                        return Err(Failure::Denied);
                    }
                    reference
                        .generation()
                        .next()
                        .ok_or(Failure::VersionExhausted)?;
                    block
                } else {
                    source
                        .native_block(
                            entry.authority(),
                            PageOffset::new(offset),
                            count,
                            start.flat(),
                            actor,
                            start.token(),
                        )
                        .map_err(pend_error)?
                };
                receipt.start(Value::Maintenance {
                    block: block.reference(),
                    phase: BlockPhase::Draining,
                });
                (entry, meta, source, block, operation)
            };
            if let Some(reference) = recover {
                block.retire_uses(None).map_err(pend_error)?;
                flat::authority::settle_block(&meta, &source, &block).map_err(pend_error)?;
                let _serial = actor.pend.lock();
                let _commit = basis::commit();
                if !pend::live(actor)
                    || !entry.permits(kernel_api::model::pend::Operations::Flat(FlatRights::BLOCK))
                {
                    return Err(Failure::Denied);
                }
                block
                    .recover(reference, actor, start.token(), entry.authority())
                    .map_err(pend_error)?;
                // Takeover is already committed even if a later bounded drain
                // fails. Reply recovery must identify the new, usable epoch.
                receipt.start(Value::Maintenance {
                    block: block.reference(),
                    phase: BlockPhase::Draining,
                });
            }
            if block.phase() != BlockPhase::Held {
                flat::authority::settle_block(&meta, &source, &block).map_err(pend_error)?;
            }
            let _commit = basis::commit();
            if !pend::live(actor) || !entry.alive() {
                return Err(Failure::Dead);
            }
            if !matches!(block.phase(), BlockPhase::Draining | BlockPhase::Held)
                || !block.matches(block.reference(), actor.ident.id, entry.authority())
                || source
                    .block_ref(block.reference())
                    .is_none_or(|known| !Arc::ptr_eq(&known, &block))
            {
                return Err(Failure::Denied);
            }
            if (block.phase() != BlockPhase::Held && !block.settled()) || source.moving() {
                return Err(Failure::Busy);
            }
            block.set_phase(BlockPhase::Held);
            let value = Value::Maintenance {
                block: block.reference(),
                phase: BlockPhase::Held,
            };
            receipt.committed(value);
            Ok(value)
        }
        Action::Allow { maintenance } => {
            let (entry, source, block, _operation) = {
                let _serial = actor.pend.lock();
                let _commit = basis::commit();
                let selected = resolve(actor, maintenance, false)?;
                let meta = selected.0.target().memory().ok_or(Failure::Denied)?;
                let operation =
                    flat::sharing::SharingOperation::acquire(&meta).ok_or(Failure::Busy)?;
                selected.2.set_phase(BlockPhase::Releasing);
                receipt.start(Value::Maintenance {
                    block: maintenance.block,
                    phase: BlockPhase::Releasing,
                });
                (selected.0, selected.1, selected.2, operation)
            };
            block.retire_uses(None).map_err(pend_error)?;
            let _commit = basis::commit();
            if !block.settled() || block.has_uses() || block.staged() || source.moving() {
                return Err(Failure::Busy);
            }
            if !pend::live(actor) || !block.owner_live() {
                return Err(Failure::Dead);
            }
            if !block.matches(maintenance.block, actor.ident.id, entry.authority()) {
                return Err(Failure::Denied);
            }
            source.allow(entry.authority());
            block.set_phase(BlockPhase::Closed);
            drop(block);
            let value = Value::Maintenance {
                block: maintenance.block,
                phase: BlockPhase::Closed,
            };
            receipt.committed(value);
            Ok(value)
        }
        _ => Err(Failure::Unsupported),
    }
}

pub(in crate::entry::call) fn resolve(
    actor: &Arc<Task>,
    maintenance: memory::Maintenance,
    held: bool,
) -> Result<
    (
        Arc<pend::PendEntry>,
        Arc<pend::SourceAuthorization>,
        Arc<pend::RangeBlock>,
    ),
    Failure,
> {
    if !pend::live(actor) {
        return Err(Failure::Dead);
    }
    let entry = pend::locate(actor, maintenance.token).ok_or(Failure::Denied)?;
    if !entry.permits(Operations::Flat(FlatRights::BLOCK)) {
        return Err(Failure::Denied);
    }
    let pend::Target::Flat(meta) = entry.target() else {
        return Err(Failure::Denied);
    };
    if meta.source().source_id() != maintenance.block.flat() {
        return Err(Failure::Denied);
    }
    let source = meta.authorization().ok_or(Failure::Denied)?;
    let block = source.blocking(entry.authority()).ok_or(Failure::Stale)?;
    if !block.matches(maintenance.block, actor.ident.id, entry.authority()) {
        return Err(Failure::Denied);
    }
    let (offset, count) = block.window();
    if !entry.authority().selects(
        Operations::Flat(FlatRights::BLOCK),
        PendScope::Pages(PendRange::Pages {
            offset: offset.get(),
            count: count.get(),
        }),
    ) {
        return Err(Failure::Denied);
    }
    if held && !matches!(block.phase(), BlockPhase::Held | BlockPhase::Releasing) {
        return Err(Failure::Busy);
    }
    Ok((entry, source, block))
}

pub(super) fn vacate(
    actor: &Arc<Task>,
    slot: kernel_api::model::flat::SlotPosition,
    expected: kernel_api::model::flat::SlotVersion,
    maintenance: memory::Maintenance,
) -> Result<Value, Failure> {
    let range = flat::authority::slot(actor, slot, memory::rights::VACATE).map_err(pend_error)?;
    let (entry, source, block) = resolve(actor, maintenance, true)?;
    let in_origin = block.covers(&range);
    if !entry.permits(Operations::Flat(memory::rights::VACATE)) {
        return Err(Failure::Denied);
    }
    if !in_origin {
        let page = range
            .page(PageOffset::ZERO)
            .map_err(|_| Failure::Busy)?
            .ok_or(Failure::SourceEmpty)?;
        if !range.staged_by(&block, page.id()) {
            return Err(Failure::Denied);
        }
    }
    let (base, count) = block.window();
    source
        .check_move(base, count, Some(entry.authority()))
        .map_err(pend_error)?;
    let target = pend::locate(actor, slot.token()).ok_or(Failure::Denied)?;
    let target_meta = target.target().memory().ok_or(Failure::Denied)?;
    let target_source = target_meta.authorization().ok_or(Failure::Denied)?;
    let offset = range
        .index()
        .get()
        .checked_sub(target_meta.source().index().get())
        .ok_or(Failure::Denied)?;
    target_source
        .check_move(
            PageOffset::new(offset),
            range.page_count(),
            Arc::ptr_eq(&source, &target_source).then_some(entry.authority().as_ref()),
        )
        .map_err(pend_error)?;
    let version = block.vacate(&range, expected).map_err(residency_error)?;
    Ok(Value::Slot { version, page: 0 })
}

/// Atomic holding movement retains the exact original block in a staged Slot.
/// The original range's TRANSFER authority remains necessary on every hop.
pub(super) fn transfer(
    actor: &Arc<Task>,
    from: kernel_api::model::flat::SlotPosition,
    from_version: kernel_api::model::flat::SlotVersion,
    to: kernel_api::model::flat::SlotPosition,
    to_version: kernel_api::model::flat::SlotVersion,
    maintenance: memory::Maintenance,
) -> Result<
    (
        kernel_api::model::flat::SlotVersion,
        kernel_api::model::flat::SlotVersion,
        basis::cell::page::PageId,
    ),
    Failure,
> {
    let from_token = from.token();
    let from = flat::authority::slot(actor, from, FlatRights::TRANSFER).map_err(pend_error)?;
    let target = flat::authority::slot(actor, to, FlatRights::TRANSFER).map_err(pend_error)?;
    let (entry, original, block) = resolve(actor, maintenance, true)?;
    if !entry.permits(Operations::Flat(FlatRights::TRANSFER)) {
        return Err(Failure::Denied);
    }
    let page = from
        .page(PageOffset::ZERO)
        .map_err(|_| Failure::Busy)?
        .ok_or(Failure::SourceEmpty)?;
    let in_origin = block.covers(&from);
    let target_origin = block.covers(&target);
    let origin_entry = pend::locate(actor, from_token).ok_or(Failure::Denied)?;
    if in_origin && !Arc::ptr_eq(origin_entry.authority(), entry.authority()) {
        return Err(Failure::Denied);
    }
    if !in_origin && from.stage_origin(&block, page.id()).is_none() {
        // Importing an untracked replacement cannot wash a restricted donor
        // branch into the destination's stronger rights. The initial import is
        // limited to the caller's unrestricted root allocation; tracked Pages
        // instead carry their original maintenance record on every hop.
        let donor = origin_entry
            .target()
            .memory()
            .and_then(|meta| meta.authorization())
            .ok_or(Failure::Denied)?;
        let root = donor.root().ok_or(Failure::Denied)?;
        let limits = origin_entry.authority().limits();
        if !Arc::ptr_eq(&root.2, origin_entry.authority())
            || origin_entry.modifiable() != kernel_api::model::modifiable::Modifiable::Yes
            || limits.distributable != kernel_api::model::pend::Distributable::Yes
            || !origin_entry.permits(Operations::Flat(
                FlatRights::ASSOCIATE | memory::rights::VACATE | FlatRights::TRANSFER,
            ))
        {
            return Err(Failure::Denied);
        }
    }
    let origin = if in_origin {
        from.index().get()
    } else if let Some(origin) = from.stage_origin(&block, page.id()) {
        origin
    } else if target_origin {
        target.index().get()
    } else {
        return Err(Failure::Denied);
    };
    // The initial import may start at an ordinary staging Page; at least one
    // endpoint must be in this record's original range or already tracked.
    let (offset, count) = block.window();
    original
        .check_move(offset, count, Some(entry.authority()))
        .map_err(pend_error)?;
    for (range, token) in [(&from, from_token), (&target, to.token())] {
        let selected = pend::locate(actor, token).ok_or(Failure::Denied)?;
        let meta = selected.target().memory().ok_or(Failure::Denied)?;
        let source = meta.authorization().ok_or(Failure::Denied)?;
        let offset = range
            .index()
            .get()
            .checked_sub(meta.source().index().get())
            .ok_or(Failure::Denied)?;
        source
            .check_move(
                PageOffset::new(offset),
                range.page_count(),
                Arc::ptr_eq(&source, &original).then_some(entry.authority().as_ref()),
            )
            .map_err(pend_error)?;
    }
    let was_retained = block.retained(&page);
    block.retain(page.clone()).map_err(pend_error)?;
    let result = (|| {
        let permit = block.permit(&page).map_err(pend_error)?;
        if page.obligations() != 0 || page.quarantined() {
            return Err(Failure::Busy);
        }
        let stage = if target_origin {
            None
        } else {
            Some(block.stage(page.id(), origin).map_err(pend_error)?)
        };
        from.transfer_held(from_version, &target, to_version, permit.change(), stage)
            .map_err(residency_error)
    })();
    if result.is_err() && !was_retained {
        block.cancel_page(&page);
    }
    result
}
