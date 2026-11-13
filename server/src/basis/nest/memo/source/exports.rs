//! Discovery-only reverse cleanup for logical exports. Callers close admission
//! first; the exact Memo binding and current Source remain the authority.
use super::*;
use crate::{basis::pend::Tracking, metal::sync::SpinLock};

/// Task and Reservation indexes refer to the same SourceAuthorization. No index
/// lock, Task ledger serial or COMMIT guard crosses a hardware retirement.
pub(crate) fn withdraw(
    sources: &SpinLock<Tracking<SourceAuthorization>>,
    selected: impl Fn(&Source) -> bool,
) -> Result<(), PendFail> {
    let mut cursor = 0;
    let mut retired = 0;
    loop {
        let source = sources.lock().after(cursor);
        let Some(source) = source else {
            return Ok(());
        };
        cursor = Arc::as_ptr(&source) as usize;
        for binding in source.bindings()? {
            let nest = binding.nest().upgrade().ok_or(PendFail::Busy)?;
            let matches = {
                let _commit = crate::basis::commit();
                nest.binding_source(&binding)
                    .is_some_and(|source| selected(&source))
            };
            if matches {
                nest.withdraw_source(&binding, &source, None, None, true)?;
                retired += 1;
                // Clear at most four matching Memos per call. The reverse
                // binding disappears only after exact retirement, so retrying
                // starts from retained records without skipping failed work.
                if retired == 4 {
                    return Err(PendFail::Busy);
                }
            }
        }
    }
}
