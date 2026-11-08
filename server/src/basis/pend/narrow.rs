use crate::basis::{unit::task::Task, view::share::ShareOperation};
use alloc::sync::Arc;
use kernel_api::{
    abi::call::pend::PendFail,
    model::{identity::PendId, modifiable::Modifiable, pend::PendRange},
};

/// Narrow preserves identity. Live descendants/external loans return Busy before
/// changing the window; the holder can Return/Revoke them and retry.
pub(crate) fn reduce(
    task: &Arc<Task>,
    token: PendId,
    subset: Modifiable,
    range: PendRange,
) -> Result<(), PendFail> {
    let observed = super::locate(task, token).ok_or(PendFail::Denied)?;
    let share = observed.view().share_view();
    let _operation = share
        .as_ref()
        .map(|meta| ShareOperation::acquire(meta).ok_or(PendFail::Busy))
        .transpose()?;
    let _serial = task.pend.lock();
    let observed = super::locate(task, token).ok_or(PendFail::Denied)?;
    if !observed.alive() {
        return Err(PendFail::Dead);
    }
    observed.authority().can_narrow(subset)?;
    if task
        .pend
        .heirs
        .lock()
        .iter()
        .any(|(parent, _, _)| *parent == token)
    {
        return Err(PendFail::Busy);
    }
    let window = match range {
        PendRange::Inherit => observed.authority().window(),
        _ => {
            let meta = share.as_ref().ok_or(PendFail::Denied)?;
            let outer = observed.authority().window().unwrap_or((
                kernel_api::address::PageOffset::ZERO,
                meta.source().page_count(),
            ));
            Some(super::Window::Parent(range).resolve(outer)?)
        }
    };
    let authorization = share.as_ref().and_then(|meta| meta.authorization());
    // No in-flight publication may cross the narrowing commit.
    {
        let _commit = crate::basis::commit();
        let _withdraw = authorization
            .as_ref()
            .map(|source| source.begin_withdraw())
            .transpose()?;
    }
    if let Some(meta) = &share {
        if subset != observed.modifiable() || window != observed.authority().window() {
            meta.close_maps(Some(token))?;
        }
    }
    {
        let _commit = crate::basis::commit();
        let _withdraw = authorization
            .as_ref()
            .map(|source| source.begin_withdraw())
            .transpose()?;
        let mut entries = task.pend.entries.lock();
        let entry = entries
            .iter_mut()
            .find(|entry| entry.token() == token)
            .ok_or(PendFail::Denied)?;
        entry.narrow(subset, window)?;
        super::changed(task);
    }
    drop(_serial);
    drop(_operation);
    super::notify(task.ident.id, token);
    Ok(())
}
