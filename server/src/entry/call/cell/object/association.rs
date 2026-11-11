use super::*;

/// Keeps the source Memo authority and generation serialized until the target
/// association commits. A readable Page/Slot observation cannot authorize this.
fn with_source<T>(
    pos: CellPosition,
    ident: &Arc<TaskIdent>,
    run: impl FnOnce(FlatRange, Arc<Page>, Option<Arc<pend::Pend>>) -> Result<T, CellFail>,
) -> Result<T, CellFail> {
    let actor = caller(ident).ok_or(CellFail::Denied)?;
    match pos.object {
        CellObject::Slot(id) => {
            let hold = actor
                .pend
                .references
                .get(id.get())
                .ok_or(CellFail::Denied)?;

            let range = hold
                .resolve()
                .ok_or(CellFail::Denied)?
                .cell(PageOffset::new(pos.page))?;
            let (base, _) = hold.window();
            hold.source()
                .check_move(
                    PageOffset::new(base.get() + pos.page),
                    range.page_count(),
                    hold.authority().map(Arc::as_ref),
                )
                .map_err(source_fail)?;
            let cell = crate::basis::cell::Cell::Flat(range.clone());
            let page = match cell.page(0) {
                Ok(page) => page,
                Err(NestError::Missing) => {
                    let _pages = range.materialize()?;
                    cell.page(0)?
                }
                Err(e) => return Err(e.into()),
            };
            let _commit = crate::basis::commit();
            if !pend::live(&actor) || hold.resolve().is_none() {
                return Err(CellFail::Denied);
            }
            hold.source()
                .check_move(
                    PageOffset::new(base.get() + pos.page),
                    range.page_count(),
                    hold.authority().map(Arc::as_ref),
                )
                .map_err(source_fail)?;
            hold.source()
                .retain_blocked(
                    PageOffset::new(base.get() + pos.page),
                    range.page_count(),
                    &page,
                )
                .map_err(source_fail)?;
            run(range, page, hold.authority().cloned())
        }
        CellObject::Memo(token) => with_memo(pos, MemoRights::INSERT, ident, |lease, page| {
            if lease.memo().nest.is_external() {
                if page != 0 {
                    return Err(CellFail::NoRegion);
                }
                let _commit = crate::basis::commit();
                let (source, data) =
                    external::page(&actor, token, lease.memo().mapping, pos.generation)?;
                run(source, data, None)
            } else {
                cpu(lease, |_, entry| {
                    check(entry, &actor)?;
                    let selected = index(entry, page)?;
                    if selected.mapping().inspect_memo(page)?.generation != pos.generation {
                        return Err(CellFail::Busy);
                    }
                    let source = selected.source()?;
                    run(
                        source,
                        crate::basis::cell::Cell::Nest(selected).page(0)?,
                        None,
                    )
                })
            }
        }),
    }
}
pub(super) fn associate(spec: CellAssociation, ident: &Arc<TaskIdent>) -> Result<usize, CellFail> {
    let actor = caller(ident).ok_or(CellFail::Denied)?;
    let _serial = actor.pend.lock();
    // Resolve the target first so an invalid destination cannot materialize a source.
    with_memo(spec.target, MemoRights::INSERT, ident, |lease, page| {
        with_source(spec.source, ident, |source, data, block_owner| {
            if lease.memo().nest.is_external() {
                if page != 0 {
                    return Err(CellFail::NoRegion);
                }
                let _commit = crate::basis::commit();
                external::associate(
                    &actor,
                    lease.0.token(),
                    lease.memo().mapping,
                    spec.target.generation,
                    &source,
                    data,
                    block_owner.as_deref(),
                )?;
                Ok(0)
            } else {
                cpu(lease, |nest, entry| {
                    check(entry, &actor)?;
                    let selected = index(entry, page)?;
                    if !selected.source()?.same_range(&source) {
                        return Err(CellFail::Denied);
                    }
                    let root = entry.source.root();
                    let _operation = root
                        .as_ref()
                        .map(|(meta, _, _)| SharingOperation::acquire(meta).ok_or(CellFail::Busy))
                        .transpose()?;
                    check(entry, &actor)?;
                    let _commit = crate::basis::commit();
                    let base = entry
                        .source
                        .full_range()
                        .ok_or(CellFail::Denied)?
                        .index()
                        .get();
                    let offset = PageOffset::new(source.index().get() - base);
                    entry
                        .source
                        .check_move(offset, source.page_count(), block_owner.as_deref())
                        .map_err(source_fail)?;
                    entry
                        .source
                        .retain_blocked(offset, source.page_count(), &data)
                        .map_err(source_fail)?;
                    nest.associate(selected.mapping(), page, spec.target.generation, data)?;
                    Ok(0)
                })
            }
        })
    })
}
