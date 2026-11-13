//! The minimal ordinary-access drain required by versioned Vacate. Maintenance
//! publication and orphan takeover remain separately gated M4 operations.
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
            recover: None,
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
                let block = source
                    .native_block(
                        entry.authority(),
                        PageOffset::new(offset),
                        count,
                        start.flat(),
                        actor.ident.id,
                    )
                    .map_err(pend_error)?;
                receipt.start(Value::Maintenance {
                    block: block.reference(),
                    phase: BlockPhase::Draining,
                });
                (entry, meta, source, block, operation)
            };
            flat::authority::settle_block(&meta, &source, &block).map_err(pend_error)?;
            let _commit = basis::commit();
            if !pend::live(actor) || !entry.alive() {
                return Err(Failure::Dead);
            }
            if !block.settled() || source.moving() {
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
            let _serial = actor.pend.lock();
            let _commit = basis::commit();
            let (entry, source, block) = resolve(actor, maintenance, false)?;
            if !block.settled() || source.moving() {
                return Err(Failure::Busy);
            }
            block.set_phase(BlockPhase::Releasing);
            receipt.start(Value::Maintenance {
                block: maintenance.block,
                phase: BlockPhase::Releasing,
            });
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

pub(super) fn resolve(
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
    if held && (block.phase() != BlockPhase::Held || !block.settled()) {
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
    let meta = entry.target().memory().ok_or(Failure::Denied)?;
    if range.source_id() != maintenance.block.flat()
        || !Arc::ptr_eq(range.flat(), meta.source().flat())
    {
        return Err(Failure::Denied);
    }
    let offset = range
        .index()
        .get()
        .checked_sub(meta.source().index().get())
        .ok_or(Failure::Denied)?;
    let (base, count) = block.window();
    if offset < base.get()
        || offset
            .checked_add(range.page_count().get())
            .is_none_or(|end| end > base.get() + count.get())
    {
        return Err(Failure::Denied);
    }
    source
        .check_blocks(
            PageOffset::new(offset),
            range.page_count(),
            Some(entry.authority()),
        )
        .map_err(pend_error)?;
    let version = block.vacate(&range, expected).map_err(residency_error)?;
    Ok(Value::Slot { version, page: 0 })
}
