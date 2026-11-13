use super::{Sharing, seal};
use alloc::sync::Arc;
use kernel_api::abi::call::pend::PendFail;

pub(crate) fn pending_close(meta: &Sharing) -> bool {
    meta.source().flat().phase() == kernel_api::model::flat::FlatPhase::Closing
        || meta.has_maps()
        || meta
            .authorization()
            .is_some_and(|source| source.pending_close())
}

/// Hardware withdrawal runs outside COMMIT and source locks.
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
    source.retire_origins(None, None)?;
    for _ in 0..4 {
        let Some(binding) = source.first_binding() else {
            break;
        };
        let space = binding.nest().upgrade().ok_or(PendFail::Busy)?;
        space.withdraw_source(&binding, &source, None, None, true)?;
    }
    if source.pending_close() {
        Err(PendFail::Busy)
    } else {
        if root.source().flat().phase() == kernel_api::model::flat::FlatPhase::Closing {
            source.close_blocks()?;
            root.source()
                .flat()
                .finish_close()
                .map_err(|_| PendFail::Busy)?;
        }
        Ok(())
    }
}
