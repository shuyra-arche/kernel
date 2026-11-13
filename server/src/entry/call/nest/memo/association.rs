use super::*;
use crate::basis::nest::memo::source::Source;
use kernel_api::model::pend::{NestScope, PendScope};

fn materialize(source: Source) -> Result<(Arc<Page>, Source), CellFail> {
    let range = source.range()?;
    let (offset, count) = source.window();
    source
        .source()
        .check_move(offset, count, source.hold().authority().map(Arc::as_ref))
        .map_err(source_fail)?;
    let page = match range.page(PageOffset::ZERO)? {
        Some(page) => page,
        None => {
            let _pages = range.materialize()?;
            range.page(PageOffset::ZERO)?.ok_or(CellFail::SourceEmpty)?
        }
    };
    source.same_page(&page)?;
    source
        .source()
        .retain_blocked(offset, count, &page)
        .map_err(source_fail)?;
    Ok((page, source))
}

fn with_source<T>(
    actor: &Arc<Task>,
    pos: CellPosition,
    count: PageCount,
    run: impl FnOnce(Arc<Page>, Source) -> Result<T, CellFail>,
) -> Result<T, CellFail> {
    match pos.object {
        CellObject::Slot(slot) => {
            let hold = actor
                .pend
                .references
                .get(slot.get())
                .ok_or(CellFail::Denied)?;
            let source = Source::new(actor, hold, PageOffset::new(pos.page), count)?;
            let (page, source) = materialize(source)?;
            run(page, source)
        }
        CellObject::Pend(token) => {
            let entry = pend::locate(actor, token).ok_or(CellFail::Denied)?;
            match entry.scope() {
                PendScope::Pages(_) => {
                    if pos.generation != 0 || !entry.associates() {
                        return Err(CellFail::Denied);
                    }
                    let base = entry.authority().window().ok_or(CellFail::Denied)?.0;
                    let offset = base
                        .get()
                        .checked_add(pos.page)
                        .ok_or(CellFail::NotAligned)?;
                    let source = Source::from_pend(actor, token, PageOffset::new(offset), count)?;
                    let (page, source) = materialize(source)?;
                    run(page, source)
                }
                PendScope::Nest(NestScope::Reservation { .. }) => {
                    let mapping = Reservation::acquire(actor, token, NestRights::INSERT)
                        .map_err(source_fail)?;
                    let page = mapping.page(pos.page).map_err(source_fail)?;
                    let span = mapping.span().map_err(source_fail)?;
                    let (data, source) = {
                        let _operation = span.operation().ok_or(CellFail::Busy)?;
                        if span.inspect_memo(page)?.generation != pos.generation {
                            return Err(CellFail::Busy);
                        }
                        let (data, source) = mapping
                            .nest()
                            .association(&span, page)?
                            .ok_or(CellFail::SourceEmpty)?;
                        if data.pages() != count.get() {
                            return Err(CellFail::NotAligned);
                        }
                        let (mut ceiling, _) = span.access(page)?;
                        if mapping.entry().modifiable() != Modifiable::Yes {
                            ceiling.remove(Access::WRITE);
                        }
                        (data, source.limited(ceiling))
                    };
                    source.same_page(&data)?;
                    run(data, source)
                }
                _ => Err(CellFail::Denied),
            }
        }
    }
}

pub(in crate::entry::call) fn associate(
    spec: CellAssociation,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    let actor = caller(ident).ok_or(CellFail::Denied)?;
    let _serial = actor.pend.lock();
    let _commit = crate::basis::commit();
    let CellObject::Pend(token) = spec.target.object else {
        return Err(CellFail::Denied);
    };
    let mapping = Reservation::acquire(&actor, token, NestRights::INSERT).map_err(source_fail)?;
    let page = mapping.page(spec.target.page).map_err(source_fail)?;
    let span = mapping.span().map_err(source_fail)?;
    let info = span.inspect_memo(page)?;
    if info.generation != spec.target.generation
        || !matches!(info.state, CellState::Vacant | CellState::Ready)
    {
        return Err(CellFail::Busy);
    }
    let count = PageCount::new(span.grain()).ok_or(CellFail::NotAligned)?;
    with_source(&actor, spec.source, count, |data, source| {
        if !pend::live(&actor)
            || !mapping
                .entry()
                .permits(kernel_api::model::pend::Operations::Nest(
                    NestRights::INSERT,
                ))
        {
            return Err(CellFail::Denied);
        }
        mapping
            .nest()
            .associate(&span, page, spec.target.generation, data, source)?;
        Ok(0)
    })
}
