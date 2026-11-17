//! Versioned namespace transactions. Admission closes under COMMIT; physical
//! release proceeds without ledger/result locks, retaining exact namespace IDs.
use super::*;
use crate::basis::nest::{Access, SpaceKind, authority::binding};
use crate::basis::nest::{
    Nest, Placement, PlacementAt, RegionKind, authority::binding::Error as BindingError,
};
use kernel_api::model::nest::CreateSpanRequest;
use kernel_api::model::{
    identity::Mark,
    modifiable::Modifiable,
    nest::{NestPhase, NestStatus, SpanId, SpanPhase},
    pend::{CreationRights, NestScope, PendScope},
};

fn error(error: BindingError) -> Failure {
    match error {
        BindingError::OutOfMemory => Failure::OutOfMemory,
        BindingError::Busy => Failure::Busy,
        BindingError::NotAligned => Failure::NotAligned,
        BindingError::AlreadyMapped => Failure::DestinationOccupied,
        BindingError::NoRegion => Failure::NoRegion,
        _ => Failure::Denied,
    }
}

pub(super) fn mutate(actor: &Arc<Task>, action: NestAction) -> Result<Value, Failure> {
    match action {
        NestAction::Reserve {
            token,
            nest: id,
            expected,
            request,
        } => {
            let parent =
                binding::authorized(actor, token, NestRights::CREATE_SPAN).map_err(error)?;
            let nest = parent.target().nest().ok_or(Failure::Denied)?;
            if nest.id() != id {
                return Err(Failure::Denied);
            }
            if nest.version() != expected {
                return Err(Failure::Stale);
            }
            expected.next().ok_or(Failure::VersionExhausted)?;
            if request.level.get() >= crate::metal::mmu::internal::mode::levels() {
                return Err(Failure::NotAligned);
            }
            let access = Access::from_wire(request.access).ok_or(Failure::Invalid)?;
            if access.contains(Access::WRITE) && parent.modifiable() != Modifiable::Yes {
                return Err(Failure::Denied);
            }
            let prepared = nest
                .prepare_span(
                    placement(&nest, request).map_err(error)?,
                    request.level,
                    access,
                    actor.ident.id,
                )
                .map_err(nest_error)?;
            let span = prepared.span();
            span.faults.lock().enable_native();
            let id =
                SpanId::from_bytes(&(span.identity() as u64).to_le_bytes()).expect("span identity");
            let grant =
                pend::prepare_reservation(actor, &parent, id, request.count).map_err(pend_error)?;
            let token = grant.entry().token();
            span.bind_authority(token);
            let span = prepared
                .publish(grant.entry().authority())
                .map_err(nest_error)?;
            grant.publish();
            Ok(Value::Span {
                token,
                nest: nest.id(),
                nest_version: nest.version(),
                span: id,
                version: span.version(),
                address: span.initial_start(),
            })
        }
        _ => Err(Failure::Unsupported),
    }
}

pub(super) fn inspect(actor: &Arc<Task>, query: NestQuery) -> Result<Observation, Failure> {
    let _serial = actor.pend.lock();
    let _commit = basis::commit();
    let value = match query {
        NestQuery::Nest { token, nest: id } => {
            let entry = binding::authorized(actor, token, NestRights::INSPECT).map_err(error)?;
            let nest = entry.target().nest().ok_or(Failure::Denied)?;
            if nest.id() != id {
                return Err(Failure::Denied);
            }
            loop {
                let version = nest.version();
                let info = binding::inspect(actor, token).map_err(error)?;
                if version == nest.version() {
                    break Snapshot::Nest(NestStatus { info, version });
                }
            }
        }
        NestQuery::Span {
            token,
            nest: id,
            span: reservation,
        } => {
            let route =
                SpanRoute::acquire(actor, token, NestRights::INSPECT).map_err(pend_error)?;
            if route.id() != reservation || route.nest().id() != id {
                return Err(Failure::Denied);
            }
            let span = route.span().map_err(pend_error)?;
            let (offset, count) = route.entry().authority().window().ok_or(Failure::Denied)?;
            Snapshot::Span(
                span.snapshot(
                    route.nest(),
                    offset,
                    count,
                    route.entry().modifiable() == Modifiable::Yes,
                )
                .map_err(nest_error)?,
            )
        }
        _ => return Err(Failure::Unsupported),
    };
    Observation::new(value).ok_or(Failure::Invalid)
}

pub(super) fn effect(
    actor: &Arc<Task>,
    action: NestAction,
    receipt: &mut crate::entry::call::next_memory::Receipt<'_>,
) -> Result<Value, Failure> {
    match action {
        NestAction::Create { license } => {
            {
                let _serial = actor.pend.lock();
                let _commit = basis::commit();
                pend::access::license(actor, license, CreationRights::NEST).map_err(pend_error)?;
            }
            // A private Nest installs trampoline PTEs and completes their fence.
            // Keep private preparation and failed cleanup outside all ledger/COMMIT guards.
            let nest = basis::unit::team::TeamBuilder::prepare_nest(SpaceKind::User)
                .map_err(nest_error)?;
            let root = Arc::try_new(
                pend::PendEntry::try_new(
                    pend::Target::Nest {
                        owner: actor.ident.id,
                        nest: nest.clone(),
                    },
                    Mark::NONE,
                    Modifiable::Yes,
                    None,
                )
                .map_err(pend_error)?,
            )
            .map_err(|_| Failure::OutOfMemory)?;
            let value = Value::CreatedNest {
                token: root.token(),
                nest: nest.id(),
                version: nest.version(),
            };
            let _serial = actor.pend.lock();
            let _commit = basis::commit();
            // The preparation is not authority: recheck live creation rights
            // at publication, and commit the journal before releasing COMMIT.
            // Retained root/Nest references outlive this guard on every error.
            pend::access::license(actor, license, CreationRights::NEST).map_err(pend_error)?;
            actor
                .pend
                .prepare(root.clone())
                .map_err(pend_error)?
                .publish();
            receipt.committed(value);
            Ok(value)
        }
        NestAction::Release {
            token,
            nest: id,
            span: reservation,
            expected,
        } => {
            let (nest, span) = {
                let _serial = actor.pend.lock();
                let _commit = basis::commit();
                if !pend::live(actor) {
                    return Err(Failure::Dead);
                }
                let entry = pend::locate(actor, token).ok_or(Failure::Denied)?;
                if !entry
                    .authority()
                    .permits(Operations::Nest(NestRights::DELETE_SPAN))
                    || !matches!(entry.scope(), PendScope::Nest(NestScope::Span { id, .. }) if id == reservation)
                {
                    return Err(Failure::Denied);
                }
                let nest = entry.target().nest().ok_or(Failure::Denied)?;
                if nest.id() != id {
                    return Err(Failure::Denied);
                }
                let span = nest.span(reservation).ok_or(Failure::Dead)?;
                let (offset, count) = entry.authority().window().ok_or(Failure::Denied)?;
                if offset.get() != 0 || count.checked_bytes() != Some(span.initial_size()) {
                    return Err(Failure::Denied);
                }
                if span.version() != expected {
                    return Err(Failure::Stale);
                }
                expected.next().ok_or(Failure::VersionExhausted)?;
                nest.begin_release(&span, expected).map_err(nest_error)?;
                receipt.start(Value::SpanLifecycle {
                    nest: id,
                    span: reservation,
                    version: span.version(),
                    phase: SpanPhase::Closing,
                });
                (nest, span)
            };
            nest.close_span(reservation).map_err(cell_error)?;
            let _commit = basis::commit();
            let value = Value::SpanLifecycle {
                nest: id,
                span: reservation,
                version: span.version(),
                phase: SpanPhase::Closed,
            };
            receipt.committed(value);
            Ok(value)
        }
        NestAction::ReleaseRange { target, expected } => {
            let token = target.range();
            let offset = target.offset();
            let count = target.count();
            let (id, reservation, span) = {
                let _serial = actor.pend.lock();
                let _commit = basis::commit();
                if !pend::live(actor) {
                    return Err(Failure::Dead);
                }
                let entry = pend::locate(actor, token).ok_or(Failure::Denied)?;
                let reservation = match entry.scope() {
                    PendScope::Nest(NestScope::Span { id, .. }) => id,
                    _ => return Err(Failure::Denied),
                };
                if !entry
                    .authority()
                    .permits(Operations::Nest(NestRights::DELETE_SPAN))
                {
                    return Err(Failure::Denied);
                }
                let nest = entry.target().nest().ok_or(Failure::Denied)?;
                let span = nest.span(reservation).ok_or(Failure::Dead)?;
                let (window_offset, window_count) =
                    entry.authority().window().ok_or(Failure::Denied)?;
                let end = offset
                    .get()
                    .checked_add(count.get())
                    .ok_or(Failure::Denied)?;
                let window_end = window_offset
                    .get()
                    .checked_add(window_count.get())
                    .ok_or(Failure::Denied)?;
                if offset.get() < window_offset.get() || end > window_end {
                    return Err(Failure::Denied);
                }
                if span.version().get() != expected.get() {
                    return Err(Failure::Stale);
                }
                expected.next().ok_or(Failure::VersionExhausted)?;
                nest.reclaim_range(reservation, offset, count)
                    .map_err(nest_error)?;
                (nest.id(), reservation, span)
            };
            let value = Value::SpanLifecycle {
                nest: id,
                span: reservation,
                version: span.version(),
                phase: SpanPhase::Open,
            };
            let _commit = basis::commit();
            receipt.committed(value);
            Ok(value)
        }
        NestAction::Delete { token, expected } => {
            let nest = {
                let _serial = actor.pend.lock();
                let _commit = basis::commit();
                let entry = binding::authorized(actor, token, NestRights::DELETE).map_err(error)?;
                let nest = entry.target().nest().ok_or(Failure::Denied)?;
                let id = nest.id();
                binding::check_delete_root(&nest, &entry).map_err(error)?;
                if nest.version() != expected {
                    return Err(Failure::Stale);
                }
                if nest.is_open() {
                    expected.next().ok_or(Failure::VersionExhausted)?;
                }
                nest.begin_delete().map_err(nest_error)?;
                receipt.start(Value::NestLifecycle {
                    nest: id,
                    version: nest.version(),
                    phase: NestPhase::Closing,
                });
                nest
            };
            if nest.is_external() {
                binding::delete(actor, token).map_err(error)?;
            } else {
                nest.retire().map_err(nest_error)?;
            }
            let _commit = basis::commit();
            let value = Value::NestLifecycle {
                nest: nest.id(),
                version: nest.version(),
                phase: NestPhase::Closed,
            };
            receipt.committed(value);
            Ok(value)
        }
        _ => Err(Failure::Unsupported),
    }
}

fn placement(nest: &Nest, request: CreateSpanRequest) -> Result<Placement, BindingError> {
    let size = request
        .count
        .checked_bytes()
        .ok_or(BindingError::NotAligned)?;
    let at = match request.placement.at() {
        kernel_api::model::cell::PlacementAt::Lowest => PlacementAt::Lowest,
        kernel_api::model::cell::PlacementAt::Highest => PlacementAt::Highest,
        kernel_api::model::cell::PlacementAt::Exact(address) => {
            // CPU reservations belong to the Team's Normal window. External
            // exact addresses use the backend geometry checked by prepare_span;
            // their Layout only records occupancy and has no dynamic windows.
            if !nest.is_external() {
                if !Nest::user_range(address, size) {
                    return Err(BindingError::NotAligned);
                }
                if !nest.window_contains(RegionKind::Normal, address, size) {
                    return Err(BindingError::NoRegion);
                }
            }
            PlacementAt::Exact(address)
        }
    };
    Placement::new(RegionKind::Normal, size, request.placement.align(), at)
        .map_err(|error| placement_error(error.into()))
}

fn placement_error(error: crate::basis::nest::NestError) -> BindingError {
    use crate::basis::nest::NestError;
    match error {
        NestError::OutOfMemory => BindingError::OutOfMemory,
        NestError::Busy | NestError::Shootdown => BindingError::Busy,
        NestError::NotAligned => BindingError::NotAligned,
        NestError::AlreadyMapped => BindingError::AlreadyMapped,
        NestError::NoRegion | NestError::Missing | NestError::NotMapped => BindingError::NoRegion,
        _ => BindingError::Denied,
    }
}
