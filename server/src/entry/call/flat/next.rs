//! Explicit Flat residency operations for the staged next provider.
mod lifecycle;
pub(in crate::entry::call) mod maintenance;
mod reshape;
use crate::basis::{self, flat, pend, unit::task::Task};
use alloc::sync::Arc;
use kernel_api::{
    abi::memory::{self, Action, Failure, Observation, Query, Snapshot, Value},
    model::pend::FlatRights,
};
pub(in crate::entry::call) fn effect(
    actor: &Arc<Task>,
    action: Action,
    receipt: &mut crate::entry::call::next_memory::Receipt<'_>,
) -> Result<Value, Failure> {
    if matches!(action, Action::DeleteFlat { .. }) {
        lifecycle::delete(actor, action, receipt)
    } else {
        maintenance::effect(actor, action, receipt)
    }
}

pub(in crate::entry::call) fn inspect(
    actor: &Arc<Task>,
    query: Query,
) -> Result<Observation, Failure> {
    let _serial = actor.pend.lock();
    let _commit = basis::commit();
    if !pend::live(actor) {
        return Err(Failure::Dead);
    }
    let snapshot = match query {
        Query::Slot(position) => {
            let range =
                flat::authority::slot(actor, position, FlatRights::INSPECT).map_err(pend_error)?;
            let mut status = range.inspect_slot().map_err(residency_error)?;
            let entry = pend::locate(actor, position.token()).ok_or(Failure::Denied)?;
            let meta = entry.target().memory().ok_or(Failure::Denied)?;
            let source = meta.authorization().ok_or(Failure::Denied)?;
            let offset = position
                .page()
                .checked_sub(meta.source().index().get())
                .ok_or(Failure::Denied)?;
            status.blocked |= source
                .check_blocks(
                    kernel_api::address::PageOffset::new(offset),
                    range.page_count(),
                    None,
                )
                .is_err();
            Snapshot::Slot(status)
        }
        Query::Flat { token, flat: id } => {
            let entry = pend::locate(actor, token).ok_or(Failure::Denied)?;
            if !entry
                .authority()
                .permits(kernel_api::model::pend::Operations::Flat(
                    FlatRights::INSPECT,
                ))
            {
                return Err(Failure::Denied);
            }
            let pend::Target::Flat(meta) = entry.target() else {
                return Err(Failure::Denied);
            };
            if meta.source().source_id() != id {
                return Err(Failure::Denied);
            }
            let (offset, count) = entry.authority().window().ok_or(Failure::Denied)?;
            Snapshot::Flat(kernel_api::model::flat::FlatStatus {
                id,
                version: meta.source().flat().revision(),
                offset: offset.get(),
                count: count.get(),
                phase: if meta.source().flat().is_explicit() {
                    meta.source().flat().phase()
                } else if meta.alive() {
                    kernel_api::model::flat::FlatPhase::Open
                } else {
                    kernel_api::model::flat::FlatPhase::Closing
                },
            })
        }
        Query::Block(maintenance) => {
            let entry = pend::locate(actor, maintenance.token).ok_or(Failure::Denied)?;
            if !entry.alive()
                || !entry.authority().limits().operations.contains(
                    kernel_api::model::pend::Operations::Flat(FlatRights::INSPECT),
                )
            {
                return Err(Failure::Denied);
            }
            let pend::Target::Flat(meta) = entry.target() else {
                return Err(Failure::Denied);
            };
            if meta.source().source_id() != maintenance.block.flat() {
                return Err(Failure::Denied);
            }
            let source = meta.authorization().ok_or(Failure::Denied)?;
            let block = source.block_ref(maintenance.block).ok_or(Failure::Stale)?;
            let (offset, count) = block.window();
            if !block.owner().descends_from(entry.authority())
                || !entry.authority().contains(offset, count)
            {
                return Err(Failure::Denied);
            }
            Snapshot::Block(source.inspect_block(&block).map_err(pend_error)?)
        }
        _ => return Err(Failure::Unsupported),
    };
    Observation::new(snapshot).ok_or(Failure::Invalid)
}

/// Mutation admission is repeated at commit; inspection observes blocked state.
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
        Action::Split {
            start,
            expected,
            parent,
            maintenance,
        }
        | Action::Merge {
            start,
            expected,
            parent,
            maintenance,
        } => reshape::run(
            actor,
            start,
            expected,
            parent,
            maintenance,
            matches!(action, Action::Split { .. }),
        ),
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
                let (local, remote, page) = if matches!(action, Action::Push { .. }) {
                    maintenance::transfer(
                        actor,
                        local,
                        local_version,
                        remote,
                        remote_version,
                        maintenance,
                    )?
                } else {
                    let (remote, local, page) = maintenance::transfer(
                        actor,
                        remote,
                        remote_version,
                        local,
                        local_version,
                        maintenance,
                    )?;
                    (local, remote, page)
                };
                return Ok(Value::Transfer {
                    local,
                    remote,
                    page: page.get() as u64,
                });
            }
            let local_range = resolve(actor, local, FlatRights::TRANSFER)?;
            let remote_range = resolve(actor, remote, FlatRights::TRANSFER)?;
            let donor = if matches!(action, Action::Push { .. }) {
                &local_range
            } else {
                &remote_range
            };
            if let Some(origin) = donor.provenance() {
                origin.admit(None).map_err(pend_error)?;
            }
            let (local, remote, page) = if matches!(action, Action::Push { .. }) {
                local_range
                    .transfer_to(
                        local_version,
                        &remote_range,
                        remote_version,
                        pend::Provenance::prepare(
                            actor,
                            local.token(),
                            &local_range,
                            remote.token(),
                            &remote_range,
                            None,
                            false,
                        )
                        .map_err(pend_error)?
                        .0,
                    )
                    .map_err(residency_error)?
            } else {
                let (remote, local, page) = remote_range
                    .transfer_to(
                        remote_version,
                        &local_range,
                        local_version,
                        pend::Provenance::prepare(
                            actor,
                            remote.token(),
                            &remote_range,
                            local.token(),
                            &local_range,
                            None,
                            false,
                        )
                        .map_err(pend_error)?
                        .0,
                    )
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
        } => maintenance::vacate(actor, slot, expected, maintenance),
        _ => Err(Failure::Unsupported),
    }
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
