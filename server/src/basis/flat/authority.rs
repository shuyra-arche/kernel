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
) -> Result<PendId, PendFail> {
    let _serial = task.pend.lock();
    pend::access::license(task, license, CreationRights::FLAT)?;
    let allocation =
        kernel_api::model::cell::Allocation::new(size, align).ok_or(PendFail::NotAligned)?;
    let size = allocation.bytes().ok_or(PendFail::NotAligned)?;
    let allocation =
        super::Allocation::new(size / PAGE_SIZE, align).map_err(|_| PendFail::NotAligned)?;
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
    source.adopt(&meta, entry.token(), entry.authority().clone())?;
    source.consume();
    meta.commit_source();
    registration.publish();
    pend::changed(task);
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
        entry.target().seal();
        (meta, operation)
    };
    crate::basis::flat::sharing::close(&meta)
}
