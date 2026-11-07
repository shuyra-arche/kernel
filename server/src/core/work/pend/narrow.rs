use super::pend_entry::Modifiable;
use crate::core::work::pend::authority::narrows;
use crate::core::work::view::share_view::ShareOperation;
use kernel_api::abi::call::pend::PendFail;

pub(crate) fn reduce(
    task: &alloc::sync::Arc<crate::core::work::unit::task::Task>,
    token: kernel_api::model::identity::PendId,
    subset: Modifiable,
) -> Result<(), PendFail> {
    reduce_locked(task, token, subset)?;
    super::notify(task.ident.id, token);
    Ok(())
}

fn reduce_locked(
    task: &alloc::sync::Arc<crate::core::work::unit::task::Task>,
    token: kernel_api::model::identity::PendId,
    subset: Modifiable,
) -> Result<(), PendFail> {
    let pend_entry = super::locate(task, token).ok_or(PendFail::Denied)?;
    if !pend_entry.alive() {
        return Err(PendFail::Dead);
    }
    if !narrows(pend_entry.modifiable(), subset) {
        return Err(PendFail::Denied);
    }
    if let Some(p) = pend_entry.view().share_view() {
        let meta = &p;
        let _operation = ShareOperation::acquire(meta).ok_or(PendFail::Busy)?;
        pend_entry.authority().can_narrow(subset)?;
        if pend_entry.authority().modifiable().is_none() {
            return Err(PendFail::Busy);
        }
        if subset != pend_entry.modifiable() {
            meta.close_maps(Some(token))?;
        }
        let _pend = task.pend.lock();
        let _commit = crate::core::work::commit();
        let mut entries = task.pend.entries.lock();
        let pend_entry = entries
            .iter_mut()
            .find(|pend_entry| pend_entry.token() == token)
            .ok_or(PendFail::Denied)?;
        pend_entry.narrow(subset)?;
        super::changed(task);
        return Ok(());
    }
    let _pend = task.pend.lock();
    let _commit = crate::core::work::commit();
    let mut entries = task.pend.entries.lock();
    let pend_entry = entries
        .iter_mut()
        .find(|pend_entry| pend_entry.token() == token)
        .ok_or(PendFail::Denied)?;
    pend_entry.narrow(subset)?;
    super::changed(task);
    Ok(())
}
