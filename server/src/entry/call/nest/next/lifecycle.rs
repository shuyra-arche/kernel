//! Versioned namespace transactions. Admission closes under COMMIT; physical
//! release proceeds without ledger/result locks, retaining exact namespace IDs.
use super::*;
use crate::basis::nest::{Access, SpaceKind, authority::binding};
use kernel_api::{
    abi::call::nest::NestFail,
    model::{
        identity::Mark,
        modifiable::Modifiable,
        nest::{NestPhase, NestStatus, ReservationId, ReservationPhase},
        pend::{CreationRights, NestScope, PendScope},
    },
};

fn error(error: NestFail) -> Failure {
    match error {
        NestFail::OutOfMemory => Failure::OutOfMemory,
        NestFail::Busy => Failure::Busy,
        NestFail::Dead => Failure::Dead,
        NestFail::NotAligned => Failure::NotAligned,
        NestFail::AlreadyMapped => Failure::DestinationOccupied,
        NestFail::NoRegion => Failure::NoRegion,
        _ => Failure::Denied,
    }
}

pub(super) fn mutate(actor: &Arc<Task>, action: Action) -> Result<Value, Failure> {
    match action {
        Action::CreateNest { license } => {
            pend::access::license(actor, license, CreationRights::NEST).map_err(pend_error)?;
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
            let token = root.token();
            actor.pend.prepare(root).map_err(pend_error)?.publish();
            Ok(Value::CreatedNest {
                token,
                nest: nest.id(),
                version: nest.version(),
            })
        }
        Action::Reserve {
            token,
            nest: id,
            expected,
            reservation,
        } => {
            let parent = binding::authorized(actor, token, NestRights::RESERVE).map_err(error)?;
            let nest = parent.target().nest().ok_or(Failure::Denied)?;
            if nest.id() != id {
                return Err(Failure::Denied);
            }
            if nest.version() != expected {
                return Err(Failure::Stale);
            }
            expected.next().ok_or(Failure::VersionExhausted)?;
            if reservation.level.get() >= crate::metal::mmu::mode::levels() {
                return Err(Failure::NotAligned);
            }
            let access = Access::from_wire(reservation.access).ok_or(Failure::Invalid)?;
            if access.contains(Access::WRITE) && parent.modifiable() != Modifiable::Yes {
                return Err(Failure::Denied);
            }
            let prepared = nest
                .reserve_reservation(
                    super::super::placement(&nest, reservation).map_err(error)?,
                    reservation.level,
                    access,
                    actor.ident.id,
                )
                .map_err(nest_error)?;
            let span = prepared.span();
            span.faults.lock().enable_native();
            let id = ReservationId::from_bytes(&(span.identity() as u64).to_le_bytes())
                .expect("reservation identity");
            let grant = pend::prepare_native_reservation(actor, &parent, id, reservation.count)
                .map_err(pend_error)?;
            let token = grant.entry().token();
            let span = prepared
                .publish(grant.entry().authority())
                .map_err(nest_error)?;
            grant.publish();
            Ok(Value::Reservation {
                token,
                nest: nest.id(),
                nest_version: nest.version(),
                reservation: id,
                version: span.version(),
                address: span.initial_start(),
            })
        }
        _ => Err(Failure::Unsupported),
    }
}

pub(super) fn inspect(actor: &Arc<Task>, query: Query) -> Result<Observation, Failure> {
    let _serial = actor.pend.lock();
    let _commit = basis::commit();
    let value = match query {
        Query::Nest { token, nest: id } => {
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
        Query::Reservation {
            token,
            nest: id,
            reservation,
        } => {
            let route =
                Reservation::acquire(actor, token, NestRights::INSPECT).map_err(pend_error)?;
            if route.id() != reservation || route.nest().id() != id {
                return Err(Failure::Denied);
            }
            let span = route.span().map_err(pend_error)?;
            let (offset, count) = route.entry().authority().window().ok_or(Failure::Denied)?;
            Snapshot::Reservation(
                route
                    .nest()
                    .reservation_snapshot(
                        &span,
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
    action: Action,
    receipt: &mut crate::entry::call::next_memory::Receipt<'_>,
) -> Result<Value, Failure> {
    match action {
        Action::Release {
            token,
            nest: id,
            reservation,
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
                    .permits(Operations::Nest(NestRights::RELEASE))
                    || !matches!(entry.scope(), PendScope::Nest(NestScope::Reservation { id, .. }) if id == reservation)
                {
                    return Err(Failure::Denied);
                }
                let nest = entry.target().nest().ok_or(Failure::Denied)?;
                if nest.id() != id {
                    return Err(Failure::Denied);
                }
                let span = nest.reservation(reservation).ok_or(Failure::Dead)?;
                let (offset, count) = entry.authority().window().ok_or(Failure::Denied)?;
                if offset.get() != 0 || count.checked_bytes() != Some(span.initial_size()) {
                    return Err(Failure::Denied);
                }
                if span.version() != expected {
                    return Err(Failure::Stale);
                }
                expected.next().ok_or(Failure::VersionExhausted)?;
                nest.begin_release(&span, expected).map_err(nest_error)?;
                receipt.start(Value::ReservationLifecycle {
                    nest: id,
                    reservation,
                    version: span.version(),
                    phase: ReservationPhase::Releasing,
                });
                (nest, span)
            };
            nest.release_reservation(reservation).map_err(cell_error)?;
            let _commit = basis::commit();
            let value = Value::ReservationLifecycle {
                nest: id,
                reservation,
                version: span.version(),
                phase: ReservationPhase::Released,
            };
            receipt.committed(value);
            Ok(value)
        }
        Action::DeleteNest {
            token,
            nest: id,
            expected,
        } => {
            let nest = {
                let _serial = actor.pend.lock();
                let _commit = basis::commit();
                let entry = binding::authorized(actor, token, NestRights::DELETE).map_err(error)?;
                let nest = entry.target().nest().ok_or(Failure::Denied)?;
                if nest.id() != id {
                    return Err(Failure::Denied);
                }
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
                nest: id,
                version: nest.version(),
                phase: NestPhase::Closed,
            };
            receipt.committed(value);
            Ok(value)
        }
        _ => Err(Failure::Unsupported),
    }
}
