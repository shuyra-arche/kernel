use alloc::sync::Arc;
use alloc::vec::Vec;

use kernel_api::abi::call::pend::PendFail;

use crate::basis::pend::SourceAuthorization;
use crate::basis::unit::space::MappingBinding;

use super::ShareView;
use super::pend_map_error;
use super::seal;

pub(crate) fn pending_close(meta: &ShareView) -> bool {
    meta.has_maps()
        || meta
            .authorization()
            .is_some_and(|source| source.pending_close())
}

/// 调用者持有根资源的操作租约；CPU 关闭不在全局提交锁下等待。
pub(crate) fn close(root: &Arc<ShareView>) -> Result<(), PendFail> {
    let Some(source) = root.authorization() else {
        return root.close_maps(None);
    };
    let bindings = {
        let _commit = crate::basis::commit();
        let bindings = source.bindings()?;
        source.revoke()?;
        seal(root);
        bindings
    };
    root.close_maps(None)?;
    close_bindings(&source, bindings)
}

fn close_bindings(
    source: &Arc<SourceAuthorization>,
    bindings: Vec<Arc<MappingBinding>>,
) -> Result<(), PendFail> {
    for binding in bindings {
        let (space, mapping) = {
            let _commit = crate::basis::commit();
            if !source.owns_binding(&binding) {
                continue;
            }
            let space = binding.space().upgrade().ok_or(PendFail::Busy)?;
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
        if !space.nest.settled(id)
            && let Err(error) = space.nest.retry_invalidation()
            && !space.nest.settled(id)
        {
            return Err(pend_map_error(error));
        }
        if !space.nest.settled(id) {
            let mapping = mapping.ok_or(PendFail::Busy)?;
            if let Err(error) = space.withdraw(mapping.whole())
                && !space.nest.settled(id)
            {
                return Err(pend_map_error(error));
            }
        }
        if !space.nest.settled(id) {
            return Err(PendFail::Busy);
        }
        space.retire_mapping(id);
    }
    Ok(())
}
