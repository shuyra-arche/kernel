//! Atomic logical association: no materialization and no captured Page. The
//! caller holds its ledger serial and COMMIT through resource/result commit.
use crate::basis::{
    self,
    nest::{Access, memo::source::Source},
    pend::{self, access::Mapping},
    unit::task::Task,
};
use alloc::sync::Arc;
use kernel_api::{
    abi::memory::{self, Action, Failure, Observation, Query, Snapshot, Value},
    address::{PageCount, PageOffset},
    model::{
        modifiable::Modifiable,
        pend::{FlatRights, NestRights, Operations},
    },
};

pub(in crate::entry::call) fn inspect(
    actor: &Arc<Task>,
    query: Query,
) -> Result<Observation, Failure> {
    let Query::Memo(position) = query else {
        return Err(Failure::Unsupported);
    };
    let _serial = actor.pend.lock();
    let _commit = basis::commit();
    let mapping =
        Mapping::acquire(actor, position.token(), NestRights::INSPECT).map_err(pend_error)?;
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
        Action::Insert {
            target,
            expected,
            source,
        } => {
            let mapping =
                Mapping::acquire(actor, target.token(), NestRights::INSERT).map_err(pend_error)?;
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
            let mapping =
                Mapping::acquire(actor, memo.token(), NestRights::REMOVE).map_err(pend_error)?;
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
            let mapping = Mapping::acquire(actor, position.token(), NestRights::PUBLISH)
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
            let (mut ceiling, _) = span.access(index).map_err(nest_error)?;
            if mapping.entry().modifiable() != Modifiable::Yes {
                ceiling.remove(Access::WRITE);
            }
            let source = source.limited(ceiling);
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
