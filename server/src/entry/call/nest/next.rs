//! Atomic logical association: no materialization and no captured Page. The
//! caller holds its ledger serial and COMMIT through resource/result commit.
mod lifecycle;
use crate::basis::{
    self,
    nest::memo::source::Source,
    pend::{self, access::Reservation},
    unit::task::Task,
};
use alloc::sync::Arc;
use kernel_api::{
    abi::memory::{self, Action, Failure, Observation, Query, Snapshot, Value},
    address::{PageCount, PageOffset},
    model::pend::{FlatRights, NestRights, Operations},
};

pub(in crate::entry::call) fn effect(
    actor: &Arc<Task>,
    action: Action,
    receipt: &mut super::super::next_memory::Receipt<'_>,
) -> Result<Value, Failure> {
    if matches!(action, Action::Release { .. } | Action::DeleteNest { .. }) {
        return lifecycle::effect(actor, action, receipt);
    }
    use basis::nest::{Access, memo::Publisher};
    use kernel_api::model::nest::MemoGeneration;
    let (position, expected, rights) = match action {
        Action::Publish {
            memo,
            expected,
            maintenance: None,
        } => (memo, expected, NestRights::PUBLISH),
        Action::Retract { memo, expected } => (memo, expected, NestRights::RETRACT),
        Action::Protect { memo, expected, .. } => (memo, expected, NestRights::PROTECT),
        _ => return Err(Failure::Unsupported),
    };
    let (route, span, index, data, publisher) = {
        let _serial = actor.pend.lock();
        let _commit = basis::commit();
        let route = Reservation::acquire(actor, position.token(), rights).map_err(pend_error)?;
        let index = route.position(position).map_err(pend_error)?;
        let span = route.span().map_err(pend_error)?;
        let info = route
            .nest()
            .inspect_logical(&span, index)
            .map_err(nest_error)?;
        if info.generation != expected {
            return Err(Failure::Stale);
        }
        expected.next().ok_or(Failure::VersionExhausted)?;
        span.version().next().ok_or(Failure::VersionExhausted)?;
        let data = if rights == NestRights::PUBLISH {
            let source = route
                .nest()
                .logical_source(&span, index, expected.get())
                .map_err(nest_error)?
                .ok_or(Failure::SourceEmpty)?;
            Some(
                source
                    .check(span.access(index).map_err(nest_error)?.1)
                    .map_err(cell_error)?
                    .page(PageOffset::ZERO)
                    .map_err(|error| match error {
                        basis::flat::FlatError::Missing => Failure::SourceEmpty,
                        basis::flat::FlatError::Busy => Failure::Busy,
                        basis::flat::FlatError::OutOfMemory => Failure::OutOfMemory,
                        basis::flat::FlatError::NotAligned => Failure::NotAligned,
                        basis::flat::FlatError::OutOfRange => Failure::Denied,
                    })?
                    .ok_or(Failure::SourceEmpty)?,
            )
        } else {
            let page = route.nest().memo_page(&span, index).map_err(nest_error)?;
            if rights == NestRights::RETRACT && page.is_none() {
                return Err(Failure::SourceEmpty);
            }
            page
        };
        let publisher = if rights == NestRights::PUBLISH {
            Some(Publisher::new(actor, route.entry()).map_err(cell_error)?)
        } else {
            None
        };
        (route, span, index, data, publisher)
    };
    let mut accepted = |generation| {
        receipt.start(Value::Memo {
            generation: MemoGeneration::new(generation).expect("Memo generation"),
        })
    };
    let result = match action {
        Action::Publish { .. } => {
            let change = basis::cell::change::acquire(data.as_ref().expect("selected Page"))
                .map_err(cell_error)?;
            route.nest().publish_memo_report(
                &span,
                index,
                expected.get(),
                change,
                publisher.expect("publish route"),
                &mut accepted,
            )
        }
        Action::Retract { .. } => {
            let change = basis::cell::change::acquire(data.as_ref().expect("selected Page"))
                .map_err(cell_error)?;
            route.nest().retract_memo_report(
                &span,
                index,
                expected.get(),
                change,
                &mut || {
                    if !pend::live(actor)
                        || pend::locate(actor, position.token())
                            .is_none_or(|held| !Arc::ptr_eq(&held, route.entry()))
                    {
                        return Err(kernel_api::abi::call::cell::CellFail::Denied);
                    }
                    route
                        .position(position)
                        .map(|_| ())
                        .map_err(super::super::memory::source_fail)
                },
                &mut accepted,
            )
        }
        Action::Protect { access, .. } => {
            let access = Access::from_wire(access).ok_or(Failure::Invalid)?;
            route.nest().protect_memo_report(
                &span,
                index,
                expected.get(),
                access,
                route.entry().authority(),
                &mut accepted,
            )
        }
        _ => unreachable!(),
    };
    result.map_err(|error| {
        if receipt.started().is_none() && span.version().next().is_none() {
            Failure::VersionExhausted
        } else {
            cell_error(error)
        }
    })?;
    if matches!(action, Action::Publish { .. }) && route.nest().is_external() {
        route
            .nest()
            .progress_external(&span)
            .map_err(|_| Failure::Busy)?;
    }
    let _commit = basis::commit();
    let value = receipt.started().ok_or(Failure::Busy)?;
    if matches!(action, Action::Publish { .. }) {
        let status = route
            .nest()
            .inspect_logical(&span, index)
            .map_err(nest_error)?;
        if status.phase != kernel_api::model::nest::MemoPhase::Published
            || value
                != (Value::Memo {
                    generation: status.generation,
                })
        {
            return Err(Failure::Busy);
        }
        if !pend::live(actor) || !route.entry().permits(Operations::Nest(NestRights::PUBLISH)) {
            return Err(Failure::Denied);
        }
        let source = route
            .nest()
            .logical_source(&span, index, status.generation.get())
            .map_err(nest_error)?
            .ok_or(Failure::SourceEmpty)?;
        source
            .check(span.access(index).map_err(nest_error)?.1)
            .map_err(cell_error)?;
    }
    receipt.committed(value);
    Ok(value)
}

pub(in crate::entry::call) fn inspect(
    actor: &Arc<Task>,
    query: Query,
) -> Result<Observation, Failure> {
    if !matches!(query, Query::Memo(_)) {
        return lifecycle::inspect(actor, query);
    }
    let Query::Memo(position) = query else {
        return Err(Failure::Unsupported);
    };
    let _serial = actor.pend.lock();
    let _commit = basis::commit();
    let mapping =
        Reservation::acquire(actor, position.token(), NestRights::INSPECT).map_err(pend_error)?;
    let index = mapping.position(position).map_err(pend_error)?;
    let span = mapping.span().map_err(pend_error)?;
    Observation::new(Snapshot::Memo(
        mapping
            .nest()
            .inspect_logical(&span, index)
            .map_err(nest_error)?,
    ))
    .ok_or(Failure::Invalid)
}

pub(in crate::entry::call) fn mutate(actor: &Arc<Task>, action: Action) -> Result<Value, Failure> {
    if !pend::live(actor) {
        return Err(Failure::Dead);
    }
    match action {
        Action::CreateNest { .. } | Action::Reserve { .. } => lifecycle::mutate(actor, action),
        Action::Insert {
            target,
            expected,
            source,
        } => {
            let mapping = Reservation::acquire(actor, target.token(), NestRights::INSERT)
                .map_err(pend_error)?;
            let index = mapping.position(target).map_err(pend_error)?;
            let span = mapping.span().map_err(pend_error)?;
            let info = mapping
                .nest()
                .inspect_logical(&span, index)
                .map_err(nest_error)?;
            if info.generation != expected {
                return Err(Failure::Stale);
            }
            let generation = expected.next().ok_or(Failure::VersionExhausted)?;
            span.version().next().ok_or(Failure::VersionExhausted)?;
            if info.source.is_some()
                && !mapping
                    .entry()
                    .permits(Operations::Nest(NestRights::REMOVE))
            {
                return Err(Failure::Denied);
            }
            let count = PageCount::new(span.grain()).ok_or(Failure::NotAligned)?;
            let selected = select(actor, source, count)?;
            mapping
                .nest()
                .associate_logical(&span, index, expected.get(), selected)
                .map_err(nest_error)?;
            Ok(Value::Memo { generation })
        }
        Action::Remove { memo, expected } => {
            let mapping = Reservation::acquire(actor, memo.token(), NestRights::REMOVE)
                .map_err(pend_error)?;
            let index = mapping.position(memo).map_err(pend_error)?;
            let span = mapping.span().map_err(pend_error)?;
            let info = mapping
                .nest()
                .inspect_logical(&span, index)
                .map_err(nest_error)?;
            if info.generation != expected {
                return Err(Failure::Stale);
            }
            let generation = expected.next().ok_or(Failure::VersionExhausted)?;
            span.version().next().ok_or(Failure::VersionExhausted)?;
            mapping
                .nest()
                .remove_logical(&span, index, expected.get())
                .map_err(nest_error)?;
            Ok(Value::Memo { generation })
        }
        _ => Err(Failure::Unsupported),
    }
}

fn select(
    actor: &Arc<Task>,
    selection: memory::Source,
    count: PageCount,
) -> Result<Source, Failure> {
    match selection {
        memory::Source::Flat { position, expected } => {
            let range = basis::flat::authority::slot(actor, position, FlatRights::ASSOCIATE)
                .map_err(pend_error)?;
            if range
                .slot_version()
                .map_err(super::super::flat::next::residency_error)?
                != expected
            {
                return Err(Failure::Stale);
            }
            let entry = pend::locate(actor, position.token()).ok_or(Failure::Denied)?;
            let meta = entry.target().memory().ok_or(Failure::Denied)?;
            let offset = position
                .page()
                .checked_sub(meta.source().index().get())
                .ok_or(Failure::Denied)?;
            Source::from_pend(actor, position.token(), PageOffset::new(offset), count)
                .map_err(cell_error)
        }
        memory::Source::Share {
            token,
            view,
            page,
            count: supplied,
        } => {
            let entry = pend::locate(actor, token).ok_or(Failure::Denied)?;
            let pend::Target::View(basis::view::View::Share(meta)) = entry.target() else {
                return Err(Failure::Denied);
            };
            if meta.id != view.get() || count.get() != supplied {
                return Err(Failure::Denied);
            }
            Source::from_pend(actor, token, PageOffset::new(page), count).map_err(cell_error)
        }
        memory::Source::Memo { position, expected } => {
            let mapping = Reservation::acquire(actor, position.token(), NestRights::PUBLISH)
                .map_err(pend_error)?;
            let index = mapping.position(position).map_err(pend_error)?;
            let span = mapping.span().map_err(pend_error)?;
            if span.grain() != count.get() {
                return Err(Failure::NotAligned);
            }
            if span.inspect_memo(index).map_err(nest_error)?.generation != expected.get() {
                return Err(Failure::Stale);
            }
            let source = mapping
                .nest()
                .logical_source(&span, index, expected.get())
                .map_err(nest_error)?
                .ok_or(Failure::SourceEmpty)?;
            let source = source.export(actor, &mapping, index).map_err(cell_error)?;
            source.range().map_err(cell_error)?;
            Ok(source)
        }
    }
}

fn pend_error(error: kernel_api::abi::call::pend::PendFail) -> Failure {
    super::super::flat::next::pend_error(error)
}
fn nest_error(error: basis::nest::NestError) -> Failure {
    use basis::nest::NestError;
    match error {
        NestError::OutOfMemory => Failure::OutOfMemory,
        NestError::Busy => Failure::Busy,
        NestError::NotAligned => Failure::NotAligned,
        NestError::Missing => Failure::SourceEmpty,
        NestError::NoRegion => Failure::NoRegion,
        _ => Failure::Denied,
    }
}
fn cell_error(error: kernel_api::abi::call::cell::CellFail) -> Failure {
    use kernel_api::abi::call::cell::CellFail;
    match error {
        CellFail::OutOfMemory => Failure::OutOfMemory,
        CellFail::Busy => Failure::Busy,
        CellFail::NotAligned => Failure::NotAligned,
        CellFail::SourceEmpty => Failure::SourceEmpty,
        CellFail::NoRegion => Failure::NoRegion,
        _ => Failure::Denied,
    }
}
