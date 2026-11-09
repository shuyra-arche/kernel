use alloc::sync::Arc;

use kernel_api::abi::call::pend::PendFail;

use crate::basis::nest::authority::RangeBinding;
use crate::basis::pend::SourceAuthorization;

use super::Sharing;
use super::pend_map_error;
use super::seal;

pub(crate) fn pending_close(meta: &Sharing) -> bool {
    meta.has_maps()
        || meta
            .authorization()
            .is_some_and(|source| source.pending_close())
}

/// 调用者持有根资源的操作租约；CPU 关闭不在全局提交锁下等待。
pub(crate) fn close(root: &Arc<Sharing>) -> Result<(), PendFail> {
    let Some(source) = root.authorization() else {
        return root.close_maps(None);
    };
    {
        let _commit = crate::basis::commit();
        source.revoke()?;
        seal(root);
    }
    root.close_maps(None)?;
    for _ in 0..4 {
        let Some(binding) = source.first_binding() else {
            return Ok(());
        };
        close_binding(&source, binding)?;
    }
    if source.pending_close() {
        Err(PendFail::Busy)
    } else {
        Ok(())
    }
}

fn close_binding(
    source: &Arc<SourceAuthorization>,
    binding: Arc<RangeBinding>,
) -> Result<(), PendFail> {
    let (space, mapping) = {
        let _commit = crate::basis::commit();
        if !source.owns_binding(&binding) {
            return Ok(());
        }
        let space = binding.nest().upgrade().ok_or(PendFail::Busy)?;
        let entry = space
            .mappings
            .registered(binding.id())
            .ok_or(PendFail::Busy)?;
        if !Arc::ptr_eq(&entry.source, source) || !Arc::ptr_eq(&entry.binding, &binding) {
            return Err(PendFail::Busy);
        }
        (space, entry.mapping().cloned())
    };
    let id = binding.id();
    if !space.settled(id)
        && let Err(error) = space.retry_invalidation()
        && !space.settled(id)
    {
        return Err(pend_map_error(error));
    }
    if !space.settled(id) {
        let mapping = mapping.ok_or(PendFail::Busy)?;
        if let Err(error) = space.detach(mapping.whole())
            && !space.settled(id)
        {
            return Err(pend_map_error(error));
        }
    }
    if !space.settled(id) {
        return Err(PendFail::Busy);
    }
    space.retire_mapping(id);
    Ok(())
}
