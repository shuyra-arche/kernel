//! Runtime Flat registration and checked Slot windows. The existing borrowing
//! owner is shared with Share, so delegation and withdrawal have one ledger.
use crate::basis::{
    flat::sharing::{Sharing, SharingOperation},
    nest::Access,
    pend::{self, SourceAuthorization},
    unit::task::Task,
};
use alloc::sync::Arc;
use kernel_api::{
    abi::call::pend::PendFail,
    address::{PAGE_SIZE, PageOffset},
    model::{
        flat::FlatInfo,
        identity::{Mark, PendId},
        modifiable::Modifiable,
        pend::{CreationRights, FlatRights, Operations},
    },
};
pub(crate) fn create(
    task: &Arc<Task>,
    license: PendId,
    size: usize,
    align: usize,
    level: usize,
) -> Result<PendId, PendFail> {
    let _serial = task.pend.lock();
    pend::access::license(task, license, CreationRights::FLAT)?;
    let level = kernel_api::model::cell::CellLevel::new(level).ok_or(PendFail::NotAligned)?;
    if level.get() >= crate::metal::mmu::mode::levels()
        || align != level.bytes().ok_or(PendFail::NotAligned)?
    {
        return Err(PendFail::NotAligned);
    }
    let allocation = kernel_api::model::cell::Allocation { size, align, level };
    let size = allocation.bytes().ok_or(PendFail::NotAligned)?;
    let allocation =
        super::Allocation::at_level(size / PAGE_SIZE, level).map_err(|_| PendFail::NotAligned)?;
    let range = super::Flat::acquire(task.ident.team.cells.clone(), allocation)
        .map_err(|_| PendFail::OutOfMemory)?
        .whole();
    let source = SourceAuthorization::new(range.clone(), Access::all(), task.ident.id)?;
    let meta = Sharing::from_source(range, size, task.ident.id)?;
    let entry = Arc::try_new(pend::PendEntry::try_new(
        pend::Target::Flat(meta.clone()),
        Mark::NONE,
        Modifiable::Yes,
        None,
    )?)
    .map_err(|_| PendFail::OutOfMemory)?;
    let registration = task.pend.prepare(entry.clone())?;
    let _commit = crate::basis::commit();
    if !pend::live(task) {
        return Err(PendFail::Dead);
    }
    meta.attach_source(&source);
    source
        .begin_withdraw()?
        .adopt(&meta, entry.token(), entry.authority().clone())?;
    source.consume();
    meta.commit_source();
    registration.publish();

    Ok(entry.token())
}
pub(crate) fn inspect(task: &Arc<Task>, token: PendId) -> Result<FlatInfo, PendFail> {
    let entry = authorized(task, token, FlatRights::INSPECT)?;
    let meta = entry.target().memory().ok_or(PendFail::Denied)?;
    let (offset, count) = entry
        .authority()
        .window()
        .unwrap_or((PageOffset::ZERO, meta.source().page_count()));
    Ok(FlatInfo {
        level: meta.source().level(),
        id: meta.source().source_id(),
        offset: offset.get(),
        count: count.get(),
    })
}
fn authorized(
    task: &Arc<Task>,
    token: PendId,
    rights: FlatRights,
) -> Result<Arc<pend::PendEntry>, PendFail> {
    let entry = pend::accede(task, token, pend::Need::Read)?;
    if !entry.permits(Operations::Flat(rights)) {
        return Err(PendFail::Denied);
    }
    pend::available(&entry)?;
    Ok(entry)
}
pub(crate) fn slice(
    task: &Arc<Task>,
    token: PendId,
    offset: usize,
    count: usize,
) -> Result<usize, PendFail> {
    let _serial = task.pend.lock();
    let _commit = crate::basis::commit();
    if !pend::live(task) {
        return Err(PendFail::Dead);
    }
    let entry = authorized(task, token, FlatRights::ASSOCIATE)?;
    let meta = entry.target().memory().ok_or(PendFail::Denied)?;
    let parent = entry
        .authority()
        .window()
        .unwrap_or((PageOffset::ZERO, meta.source().page_count()));
    let (offset, count) =
        pend::Window::Parent(kernel_api::model::pend::PendRange::Pages { offset, count })
            .resolve(parent)?;
    meta.source()
        .range(offset, count)
        .map_err(|_| PendFail::NotAligned)?;
    let source = meta.authorization().ok_or(PendFail::Denied)?;
    source.ensure_internal()?;
    task.pend.references.insert(
        source,
        offset,
        count,
        Access::all(),
        Some((token, entry.authority().clone())),
    )
}
pub(crate) fn delete(task: &Arc<Task>, token: PendId) -> Result<(), PendFail> {
    let (meta, _operation) = {
        let _serial = task.pend.lock();
        let entry = pend::locate(task, token).ok_or(PendFail::Denied)?;
        if !matches!(entry.target(), pend::Target::Flat(_))
            || !entry.authority().alive()
            || !entry
                .authority()
                .limits()
                .operations
                .contains(Operations::Flat(FlatRights::DELETE))
        {
            return Err(PendFail::Denied);
        }
        if entry.target().alive() && !entry.permits(Operations::Flat(FlatRights::DELETE)) {
            return Err(PendFail::Denied);
        }
        let meta = entry.target().memory().ok_or(PendFail::Denied)?;
        let operation = SharingOperation::acquire(&meta).ok_or(PendFail::Busy)?;
        let _commit = crate::basis::commit();
        entry.target().seal()?;
        (meta, operation)
    };
    crate::basis::flat::sharing::close(&meta)
}

/// First commit admission closure, then withdraw hardware outside COMMIT. Busy
/// keeps the branch-owned record and all Page gates so a retry cannot reopen it.
pub(crate) fn block(task: &Arc<Task>, token: PendId) -> Result<(), PendFail> {
    let _serial = task.pend.lock();
    let entry = control(task, token)?;
    let meta = entry.target().memory().ok_or(PendFail::Denied)?;
    let source = meta.authorization().ok_or(PendFail::Denied)?;
    let (offset, count) = entry
        .authority()
        .window()
        .unwrap_or((PageOffset::ZERO, meta.source().page_count()));
    let block = source.block(entry.authority(), offset, count)?;
    let _operation = SharingOperation::acquire(&meta).ok_or(PendFail::Busy)?;
    settle_block(&meta, &source, &block)?;
    if entry.alive() {
        Ok(())
    } else {
        Err(PendFail::Dead)
    }
}
/// A closed branch still owns its block until its original hardware users have
/// drained. The caller already serializes the Sharing operation.
pub(crate) fn retire_block(
    meta: &Arc<Sharing>,
    authority: &Arc<pend::Pend>,
) -> Result<(), PendFail> {
    let Some(source) = meta.authorization() else {
        return Ok(());
    };
    if let Some(block) = source.blocking(authority) {
        settle_block(meta, &source, &block)?;
        source.allow(authority);
    }
    Ok(())
}
fn settle_block(
    meta: &Arc<Sharing>,
    source: &Arc<SourceAuthorization>,
    block: &Arc<pend::RangeBlock>,
) -> Result<(), PendFail> {
    if source.moving() {
        return Err(PendFail::Busy);
    }
    let (offset, count) = block.window();
    let selected = meta
        .source()
        .range(offset, count)
        .map_err(|_| PendFail::NotAligned)?;
    for (_, page) in selected.materialized_cells().map_err(|_| PendFail::Busy)? {
        block.retain(page)?;
    }
    for binding in source.bindings()? {
        let Some(nest) = binding.nest().upgrade() else {
            return Err(PendFail::Busy);
        };
        let Some(record) = nest.mappings.registered(binding.id()) else {
            continue;
        };
        let Some(span) = record.mapping() else {
            return Err(PendFail::Busy);
        };
        let _mapping = span.operation().ok_or(PendFail::Busy)?;
        nest.retry_invalidation().map_err(|_| PendFail::Busy)?;
        let base = span.source().index().get();
        let low = selected.index().get().max(base);
        let high = (selected.index().get() + selected.page_count().get())
            .min(base + span.source().page_count().get());
        for index in (low..high).step_by(selected.grain()) {
            let page = index - base;
            if let Some(data) = nest.memo_page(span, page).map_err(|_| PendFail::Busy)? {
                block.retain(data.clone())?;
                nest.unpublish_page(
                    span,
                    kernel_api::address::VirtAddr::new(
                        span.initial_start().get() + page * PAGE_SIZE,
                    ),
                    &data,
                )
                .map_err(|_| PendFail::Busy)?;
                nest.finish_memo(span, page);
            }
        }
    }
    crate::basis::nest::authority::external::block_source(&source, &selected, &block)?;
    if block.settled() {
        Ok(())
    } else {
        Err(PendFail::Busy)
    }
}
pub(crate) fn allow(task: &Arc<Task>, token: PendId) -> Result<(), PendFail> {
    let _serial = task.pend.lock();
    let _commit = crate::basis::commit();
    let entry = control(task, token)?;
    let source = entry
        .target()
        .memory()
        .and_then(|m| m.authorization())
        .ok_or(PendFail::Denied)?;
    source.allow(entry.authority());
    Ok(())
}

/// Admission control is independent of temporary data-access suspension by an
/// External borrower. Only the live Flat branch and its BLOCK bit authorize it.
fn control(task: &Arc<Task>, token: PendId) -> Result<Arc<pend::PendEntry>, PendFail> {
    if !pend::live(task) {
        return Err(PendFail::Dead);
    }
    let entry = pend::locate(task, token).ok_or(PendFail::Denied)?;
    if !entry.alive()
        || !matches!(entry.target(), pend::Target::Flat(_))
        || !entry
            .authority()
            .limits()
            .operations
            .contains(Operations::Flat(FlatRights::BLOCK))
    {
        return Err(PendFail::Denied);
    }
    Ok(entry)
}
