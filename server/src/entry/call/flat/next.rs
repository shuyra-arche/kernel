//! Explicit Flat residency operations for the staged next provider.
use crate::basis::{self, flat, pend, unit::task::Task};
use alloc::sync::Arc;
use kernel_api::{
    abi::memory::{self, Action, Failure, Observation, Query, Snapshot, Value},
    model::pend::FlatRights,
};

pub(in crate::entry::call) fn inspect(
    actor: &Arc<Task>,
    query: Query,
) -> Result<Observation, Failure> {
    let Query::Slot(position) = query else {
        return Err(Failure::Unsupported);
    };
    let _serial = actor.pend.lock();
    let _commit = basis::commit();
    if !pend::live(actor) {
        return Err(Failure::Dead);
    }
    let range = resolve(actor, position, FlatRights::INSPECT)?;
    Observation::new(Snapshot::Slot(
        range.inspect_slot().map_err(residency_error)?,
    ))
    .ok_or(Failure::Invalid)
}

/// Selection is repeated at commit. In this first slice blocked or hardware-busy
/// observations fail rather than reporting fabricated zero obligations.
fn resolve(
    actor: &Arc<Task>,
    position: kernel_api::model::flat::SlotPosition,
    rights: FlatRights,
) -> Result<flat::FlatRange, Failure> {
    let range = flat::authority::slot(actor, position, rights).map_err(pend_error)?;
    let entry = pend::locate(actor, position.token()).ok_or(Failure::Denied)?;
    let meta = entry.target().memory().ok_or(Failure::Denied)?;
    let source = meta.authorization().ok_or(Failure::Denied)?;
    let offset = position
        .page()
        .checked_sub(meta.source().index().get())
        .ok_or(Failure::Denied)?;
    source
        .check_publish(
            kernel_api::address::PageOffset::new(offset),
            range.page_count(),
        )
        .map_err(pend_error)?;
    Ok(range)
}

pub(in crate::entry::call) fn prepare(
    actor: &Arc<Task>,
    action: Action,
) -> Result<Option<crate::basis::cell::page::Page>, Failure> {
    let Action::Occupy { slot, expected } = action else {
        return Ok(None);
    };
    let range = {
        let _commit = basis::commit();
        let range = resolve(actor, slot, memory::rights::OCCUPY)?;
        let snapshot = range.inspect_slot().map_err(residency_error)?;
        if snapshot.version != expected {
            return Err(Failure::Stale);
        }
        match snapshot.phase {
            kernel_api::model::flat::SlotPhase::Fresh => {}
            kernel_api::model::flat::SlotPhase::Missing => return Err(Failure::SourceEmpty),
            _ => return Err(Failure::DestinationOccupied),
        }
        range
    };
    range.prepare_occupy().map(Some).map_err(|e| match e {
        flat::FlatError::OutOfMemory => Failure::OutOfMemory,
        _ => Failure::NotAligned,
    })
}

pub(in crate::entry::call) fn mutate(
    actor: &Arc<Task>,
    action: Action,
    prepared: Option<crate::basis::cell::page::Page>,
) -> Result<Value, Failure> {
    if !pend::live(actor) {
        return Err(Failure::Dead);
    }
    match action {
        Action::Push {
            local,
            local_version,
            remote,
            remote_version,
            maintenance,
        }
        | Action::Pull {
            local,
            local_version,
            remote,
            remote_version,
            maintenance,
        } => {
            if let Some(maintenance) = maintenance {
                require_maintenance(actor, maintenance)?;
            }
            let local_range = resolve(actor, local, FlatRights::TRANSFER)?;
            let remote_range = resolve(actor, remote, FlatRights::TRANSFER)?;
            let (local, remote, page) = if matches!(action, Action::Push { .. }) {
                local_range
                    .transfer_to(local_version, &remote_range, remote_version)
                    .map_err(residency_error)?
            } else {
                let (remote, local, page) = remote_range
                    .transfer_to(remote_version, &local_range, local_version)
                    .map_err(residency_error)?;
                (local, remote, page)
            };
            Ok(Value::Transfer {
                local,
                remote,
                page: page.get() as u64,
            })
        }
        Action::CreateFlat {
            license,
            count,
            level,
        } => {
            let (token, flat, version) =
                flat::authority::create_explicit(actor, license, count, level)
                    .map_err(pend_error)?;
            Ok(Value::CreatedFlat {
                token,
                flat,
                version,
            })
        }
        Action::Occupy { slot, expected } => {
            let range = resolve(actor, slot, memory::rights::OCCUPY)?;
            let (version, page) = range
                .occupy(expected, prepared.ok_or(Failure::Invalid)?)
                .map_err(residency_error)?;
            Ok(Value::Slot {
                version,
                page: page.get() as u64,
            })
        }
        Action::Vacate {
            slot,
            expected,
            maintenance,
        } => {
            let range =
                flat::authority::slot(actor, slot, memory::rights::VACATE).map_err(pend_error)?;
            require_maintenance(actor, maintenance)?;
            let version = range.vacate(expected).map_err(residency_error)?;
            Ok(Value::Slot { version, page: 0 })
        }
        _ => Err(Failure::Unsupported),
    }
}

/// M4 owns the live BlockRef record and consumption of its retained Page gates.
/// No descriptor, numeric Page identity or caller-supplied flag can stand in for it.
fn require_maintenance(_: &Arc<Task>, _: memory::Maintenance) -> Result<(), Failure> {
    Err(Failure::Unsupported)
}

pub(in crate::entry::call) fn pend_error(error: kernel_api::abi::call::pend::PendFail) -> Failure {
    use kernel_api::abi::call::pend::PendFail;
    match error {
        PendFail::OutOfMemory => Failure::OutOfMemory,
        PendFail::Busy => Failure::Busy,
        PendFail::NotAligned => Failure::NotAligned,
        PendFail::Dead => Failure::Dead,
        _ => Failure::Denied,
    }
}
pub(in crate::entry::call) fn residency_error(error: flat::residency::Error) -> Failure {
    use flat::residency::Error;
    match error {
        Error::Unsupported => Failure::Unsupported,
        Error::Stale => Failure::Stale,
        Error::Exhausted => Failure::VersionExhausted,
        Error::OutOfMemory => Failure::OutOfMemory,
        Error::Occupied => Failure::DestinationOccupied,
        Error::Empty => Failure::SourceEmpty,
        Error::Busy => Failure::Busy,
    }
}
