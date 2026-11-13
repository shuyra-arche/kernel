use super::*;

/// Prepare the replacement authority before consuming any physical receipt.
pub(in crate::entry::call) fn reshape(
    slot: SlotId,
    merging: bool,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    let actor = caller(ident).ok_or(CellFail::Denied)?;
    let _serial = actor.pend.lock();
    let commit = crate::basis::commit();
    if !pend::live(&actor) {
        return Err(CellFail::Denied);
    }
    let hold = actor
        .pend
        .references
        .get(slot.get())
        .ok_or(CellFail::Denied)?;
    hold.source().reshape_ready(ident.id).map_err(source_fail)?;
    let range = hold.resolve().ok_or(CellFail::Denied)?;
    if range.index().get() != 0 || range.page_count() != range.flat().page_count() {
        return Err(CellFail::Denied);
    }
    let level = if merging {
        range.level().parent()
    } else {
        range.level().child()
    }
    .ok_or(CellFail::NotAligned)?;
    if level.get() >= crate::metal::mmu::mode::levels() {
        return Err(CellFail::NotAligned);
    }
    // The owner's ledger is the exclusive lease: reshape_ready admitted one
    // private Hold with no bindings or in-flight publication. Other operations
    // on this owner cannot create a second access while the ledger is retained.
    drop(commit);
    let target = Flat::acquire(
        ident.team.cells.clone(),
        crate::basis::flat::Allocation::at_level(range.page_count().get(), level)?,
    )?;
    let source =
        SourceAuthorization::new(target.whole(), hold.ceiling(), ident.id).map_err(source_fail)?;
    let commit = crate::basis::commit();
    if !pend::live(&actor) {
        return Err(CellFail::Denied);
    }
    hold.source().reshape_ready(ident.id).map_err(source_fail)?;
    let replacement = actor
        .pend
        .references
        .insert(
            source,
            PageOffset::ZERO,
            range.page_count(),
            hold.ceiling(),
            None,
        )
        .map_err(source_fail)?;
    // Reserve the replacement before moving receipts, so success cannot be
    // followed by a fallible publication allocation. No external authority can
    // see either private source through this owner's held ledger.
    drop(commit);
    let result = range.flat().reshape_into(&target);
    let _commit = crate::basis::commit();
    if let Err(error) = result {
        actor.pend.references.remove(replacement);
        return Err(error.into());
    }
    hold.source()
        .revoke()
        .expect("exclusive source has no in-flight publication");
    actor.pend.references.remove(slot.get());
    // Stop may close the Task during the private work. Its ledger still owns
    // the replacement receipts for retirement; do not publish caller success.
    if pend::live(&actor) {
        Ok(replacement)
    } else {
        Err(CellFail::Denied)
    }
}
