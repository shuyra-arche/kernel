//! Whole-Flat closure retains the same source and physical cleanup owners.
use super::*;
use kernel_api::{
    address::PageOffset,
    model::{flat::FlatPhase, pend::Operations},
};

pub(super) fn delete(
    actor: &Arc<Task>,
    action: Action,
    receipt: &mut crate::entry::call::next_memory::Receipt<'_>,
) -> Result<Value, Failure> {
    let Action::DeleteFlat {
        token,
        flat: id,
        expected,
    } = action
    else {
        return Err(Failure::Invalid);
    };
    let (meta, version, _operation) = {
        let _serial = actor.pend.lock();
        let _commit = basis::commit();
        if !pend::live(actor) {
            return Err(Failure::Dead);
        }
        let entry = pend::locate(actor, token).ok_or(Failure::Denied)?;
        if !entry
            .authority()
            .permits(Operations::Flat(FlatRights::DELETE))
        {
            return Err(Failure::Denied);
        }
        let pend::Target::Flat(meta) = entry.target() else {
            return Err(Failure::Denied);
        };
        let range = meta.source();
        if range.source_id() != id
            || range.index().get() != 0
            || range.page_count() != range.flat().page_count()
            || entry.authority().window() != Some((PageOffset::ZERO, range.page_count()))
        {
            return Err(Failure::Denied);
        }
        if range.flat().revision() != expected {
            return Err(Failure::Stale);
        }
        if !range.flat().is_explicit() {
            return Err(Failure::Unsupported);
        }
        if range.flat().phase() == FlatPhase::Open {
            expected.next().ok_or(Failure::VersionExhausted)?;
        }
        let operation = flat::sharing::SharingOperation::acquire(&meta).ok_or(Failure::Busy)?;
        let source = meta.authorization().ok_or(Failure::Denied)?;
        source.revoke().map_err(pend_error)?;
        let version = range
            .flat()
            .begin_close(expected)
            .map_err(residency_error)?;
        flat::sharing::seal(&meta);
        receipt.start(Value::FlatLifecycle {
            flat: id,
            version,
            phase: FlatPhase::Closing,
        });
        (meta.clone(), version, operation)
    };
    flat::sharing::close(&meta).map_err(pend_error)?;
    let _commit = basis::commit();
    let value = Value::FlatLifecycle {
        flat: id,
        version,
        phase: FlatPhase::Closed,
    };
    receipt.committed(value);
    Ok(value)
}
