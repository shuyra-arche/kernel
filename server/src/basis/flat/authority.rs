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
    create_kind(task, license, size, align, level, false)
}

/// Caller holds its ledger serial and COMMIT through result recording.
pub(crate) fn create_explicit(
    task: &Arc<Task>,
    license: PendId,
    count: usize,
    level: kernel_api::model::cell::CellLevel,
) -> Result<
    (
        PendId,
        kernel_api::model::flat::FlatId,
        kernel_api::model::flat::FlatVersion,
    ),
    PendFail,
> {
    let size = count.checked_mul(PAGE_SIZE).ok_or(PendFail::NotAligned)?;
    let token = create_kind(
        task,
        license,
        size,
        level.bytes().ok_or(PendFail::NotAligned)?,
        level.get(),
        true,
    )?;
    let entry = pend::locate(task, token).expect("just published Flat");
    let range = entry
        .target()
        .memory()
        .expect("Flat target")
        .source()
        .clone();
    Ok((token, range.source_id(), range.flat().revision()))
}

fn create_kind(
    task: &Arc<Task>,
    license: PendId,
    size: usize,
    align: usize,
    level: usize,
    explicit: bool,
) -> Result<PendId, PendFail> {
    let level = kernel_api::model::cell::CellLevel::new(level).ok_or(PendFail::NotAligned)?;
    let allocation = kernel_api::model::cell::Allocation { size, align, level };
    let size = allocation.bytes().ok_or(PendFail::NotAligned)?;
    let (range, source) = allocate_kind(task, license, allocation, explicit)?;
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
    pend::access::license(task, license, CreationRights::FLAT)?;
    let claim = source.claim()?;
    meta.attach_source(&source);
    source
        .begin_withdraw()?
        .adopt(&meta, entry.token(), entry.authority().clone())?;
    claim.commit();
    registration.publish();

    Ok(entry.token())
}

/// One preparation path for Flat creation and the v10 Cell compatibility entry.
/// Both callers hold the Task ledger serial; the caller that publishes later
/// rechecks the license under COMMIT. Preparation exposes no Pend or Slot.
pub(crate) fn allocate(
    task: &Arc<Task>,
    license: PendId,
    allocation: kernel_api::model::cell::Allocation,
) -> Result<(super::FlatRange, Arc<SourceAuthorization>), PendFail> {
    allocate_kind(task, license, allocation, false)
}

fn allocate_kind(
    task: &Arc<Task>,
    license: PendId,
    allocation: kernel_api::model::cell::Allocation,
    explicit: bool,
) -> Result<(super::FlatRange, Arc<SourceAuthorization>), PendFail> {
    pend::access::license(task, license, CreationRights::FLAT)?;
    if allocation.level.get() >= crate::metal::mmu::mode::levels()
        || allocation.align != allocation.level.bytes().ok_or(PendFail::NotAligned)?
    {
        return Err(PendFail::NotAligned);
    }
    let pages = allocation.bytes().ok_or(PendFail::NotAligned)? / PAGE_SIZE;
    let allocation =
        super::Allocation::at_level(pages, allocation.level).map_err(|_| PendFail::NotAligned)?;
    let mut flat = super::Flat::acquire(task.ident.team.cells.clone(), allocation)
        .map_err(|_| PendFail::OutOfMemory)?;
    if explicit {
        Arc::get_mut(&mut flat).expect("private Flat").explicit();
    }
    let range = flat.whole();
    let source = SourceAuthorization::new(range.clone(), Access::all(), task.ident.id)?;
    Ok((range, source))
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

/// Resolve one complete unit from the Flat's original origin. The descriptor
/// supplies identity, never authority. Keep the caller's ledger serial and
/// COMMIT through use; retaining the returned range is not a permit.
pub(crate) fn slot(
    task: &Arc<Task>,
    position: kernel_api::model::flat::SlotPosition,
    rights: FlatRights,
) -> Result<super::FlatRange, PendFail> {
    // BLOCK/INSPECT govern maintenance, not data access. An outstanding
    // External loan can suspend reads while its owner still closes the phase.
    let entry = if rights == FlatRights::BLOCK || rights == FlatRights::INSPECT {
        let entry = pend::locate(task, position.token()).ok_or(PendFail::Denied)?;
        if !pend::live(task)
            || !entry.alive()
            || !entry
                .authority()
                .limits()
                .operations
                .contains(Operations::Flat(rights))
        {
            return Err(PendFail::Denied);
        }
        entry
    } else {
        authorized(task, position.token(), rights)?
    };
    let pend::Target::Flat(meta) = entry.target() else {
        return Err(PendFail::Denied);
    };
    let source = meta.source();
    if position.flat() != source.source_id() {
        return Err(PendFail::Denied);
    }
    let offset = position
        .page()
        .checked_sub(source.index().get())
        .ok_or(PendFail::Denied)?;
    let count = source
        .flat()
        .level_at(kernel_api::address::PageIndex::new(position.page()))
        .pages()
        .ok_or(PendFail::NotAligned)?;
    if !matches!(
        entry.scope(),
        kernel_api::model::pend::PendScope::Pages(kernel_api::model::pend::PendRange::Pages { .. })
    ) || !entry.authority().contains(
        PageOffset::new(offset),
        kernel_api::address::PageCount::new(count).ok_or(PendFail::NotAligned)?,
    ) {
        return Err(PendFail::Denied);
    }
    source
        .range(
            PageOffset::new(offset),
            kernel_api::address::PageCount::new(count).ok_or(PendFail::NotAligned)?,
        )
        .map_err(|_| PendFail::NotAligned)
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
        if block.native() {
            block.retire_uses(None)?;
        }
        settle_block(meta, &source, &block)?;
        if !block.native() {
            source.allow(authority);
        }
    }
    Ok(())
}
pub(crate) fn settle_block(
    meta: &Arc<Sharing>,
    source: &Arc<SourceAuthorization>,
    block: &Arc<pend::RangeBlock>,
) -> Result<(), PendFail> {
    if source.moving() {
        return Err(PendFail::Busy);
    }
    source.retain_staged(block)?;
    let (offset, count) = block.window();
    let selected = meta
        .source()
        .range(offset, count)
        .map_err(|_| PendFail::NotAligned)?;
    for (_, page) in selected.materialized_cells().map_err(|_| PendFail::Busy)? {
        block.retain(page)?;
    }
    for binding in source.bindings()? {
        if binding.page().is_some() {
            continue;
        }
        let Some(nest) = binding.nest().upgrade() else {
            return Err(PendFail::Busy);
        };
        let Some(record) = nest.reservations.registered(binding.id()) else {
            continue;
        };
        let Some(span) = record.reservation() else {
            return Err(PendFail::Busy);
        };
        let _mapping = span.operation().ok_or(PendFail::Busy)?;
        nest.retry_invalidation().map_err(|_| PendFail::Busy)?;
        let Some(affine) = span.source() else {
            continue;
        };
        let base = affine.index().get();
        let low = selected.index().get().max(base);
        let high = (selected.index().get() + selected.page_count().get())
            .min(base + affine.page_count().get());
        // Affine legacy Memos keep the Reservation's original physical unit.
        // Native mixed Flat geometry is represented by per-position bindings.
        for index in (low..high).step_by(span.grain()) {
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
    crate::basis::nest::authority::withdraw_source_window(source, block)?;
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
    if source
        .blocking(entry.authority())
        .is_some_and(|block| block.native())
    {
        return Err(PendFail::Denied);
    }
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
