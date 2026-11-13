use crate::{
    basis::{
        nest::{
            Access, Nest, NestError, Placement, PlacementAt, RegionKind, SpaceKind,
            authority::binding,
        },
        pend,
        unit::task::{Task, TaskIdent},
    },
    metal::switcher::context::{Gprs, TrapContext},
};
use alloc::sync::Arc;
use kernel_api::{
    abi::call::{
        cell::CellFail,
        nest::{NestCall, NestFail},
        pend::PendFail,
    },
    address::{PAGE_SIZE, VirtAddr},
    model::{
        cell::MappingId,
        identity::{Mark, PendId},
        modifiable::Modifiable,
        nest::ReserveRequest,
        pend::{CreationRights, NestRights, NestScope, Operations, PendScope},
    },
};

fn pend_fail(error: PendFail) -> NestFail {
    match error {
        PendFail::OutOfMemory => NestFail::OutOfMemory,
        PendFail::Busy => NestFail::Busy,
        PendFail::Dead => NestFail::Dead,
        PendFail::NotAligned => NestFail::NotAligned,
        _ => NestFail::Denied,
    }
}

fn nest_fail(error: NestError) -> NestFail {
    match error {
        NestError::OutOfMemory => NestFail::OutOfMemory,
        NestError::Busy | NestError::Shootdown => NestFail::Busy,
        NestError::NotAligned => NestFail::NotAligned,
        NestError::AlreadyMapped => NestFail::AlreadyMapped,
        NestError::NoRegion | NestError::Missing | NestError::NotMapped => NestFail::NoRegion,
        _ => NestFail::Denied,
    }
}

fn cell_fail(error: CellFail) -> NestFail {
    match error {
        CellFail::OutOfMemory => NestFail::OutOfMemory,
        CellFail::Busy => NestFail::Busy,
        CellFail::NotAligned => NestFail::NotAligned,
        CellFail::AlreadyMapped => NestFail::AlreadyMapped,
        CellFail::NoRegion => NestFail::NoRegion,
        _ => NestFail::Denied,
    }
}

fn placement(nest: &Nest, request: ReserveRequest) -> Result<Placement, NestFail> {
    let size = request.count.checked_bytes().ok_or(NestFail::NotAligned)?;
    let at = match request.placement.at() {
        kernel_api::model::cell::PlacementAt::Lowest => PlacementAt::Lowest,
        kernel_api::model::cell::PlacementAt::Highest => PlacementAt::Highest,
        kernel_api::model::cell::PlacementAt::Exact(address) => {
            // CPU reservations belong to the Team's Normal window. External
            // exact addresses use the backend geometry checked by reserve_mapping;
            // their Layout only records occupancy and has no dynamic windows.
            if !nest.is_external() {
                if !Nest::user_range(address, size) {
                    return Err(NestFail::NotAligned);
                }
                if !nest.window_contains(RegionKind::Normal, address, size) {
                    return Err(NestFail::NoRegion);
                }
            }
            PlacementAt::Exact(address)
        }
    };
    Placement::new(RegionKind::Normal, size, request.placement.align(), at)
        .map_err(|error| nest_fail(error.into()))
}

fn reserve(
    actor: &Arc<Task>,
    token: PendId,
    at: VirtAddr,
    ident: &Arc<TaskIdent>,
) -> Result<(PendId, VirtAddr), NestFail> {
    let mut bytes = [0; ReserveRequest::WIDTH];
    if !ident.team.nest.copy_in(&mut bytes, at) {
        return Err(NestFail::Denied);
    }
    let request = ReserveRequest::from_bytes(&bytes).ok_or(NestFail::NotAligned)?;
    if request.level.get() >= crate::metal::mmu::mode::levels() {
        return Err(NestFail::NotAligned);
    }
    let access = Access::from_wire(request.access).ok_or(NestFail::NotAligned)?;
    let _serial = actor.pend.lock();
    let _commit = crate::basis::commit();
    let parent = pend::locate(actor, token).ok_or(NestFail::Denied)?;
    if !pend::live(actor)
        || parent.scope() != PendScope::Nest(NestScope::Whole)
        || !parent.permits(Operations::Nest(NestRights::RESERVE))
        || access.contains(Access::WRITE) && parent.modifiable() != Modifiable::Yes
    {
        return Err(NestFail::Denied);
    }
    let nest = parent.target().nest().ok_or(NestFail::Denied)?;
    let reserved = nest
        .reserve_mapping(
            placement(&nest, request)?,
            request.level,
            access,
            actor.ident.id,
        )
        .map_err(nest_fail)?;
    let span = reserved.span();
    let id = MappingId::from_bytes(&(span.identity() as u64).to_le_bytes())
        .filter(|id| *id != MappingId::NONE)
        .ok_or(NestFail::OutOfMemory)?;
    let address = span.initial_start();
    let grant: pend::PreparedMapping<'_> =
        pend::prepare_mapping(actor, &parent, id, request.count).map_err(pend_fail)?;
    let result = grant.entry().token();
    if !pend::live(actor)
        || !parent.permits(Operations::Nest(NestRights::RESERVE))
        || !nest.is_open()
    {
        return Err(NestFail::Dead);
    }
    reserved
        .publish(grant.entry().authority())
        .map_err(nest_fail)?;
    grant.publish();
    Ok((result, address))
}

fn release(actor: &Arc<Task>, token: PendId) -> Result<usize, NestFail> {
    let _serial = actor.pend.lock();
    let (nest, id) = {
        let _commit = crate::basis::commit();
        if !pend::live(actor) {
            return Err(NestFail::Dead);
        }
        let entry = pend::locate(actor, token).ok_or(NestFail::Denied)?;
        let PendScope::Nest(NestScope::Mapping { id, .. }) = entry.scope() else {
            return Err(NestFail::Denied);
        };
        if !entry
            .authority()
            .permits(Operations::Nest(NestRights::RELEASE))
        {
            return Err(NestFail::Denied);
        }
        let nest = entry.target().nest().ok_or(NestFail::Denied)?;
        let span = nest.mapping(id).ok_or(NestFail::Dead)?;
        let (offset, count) = entry.authority().window().ok_or(NestFail::Denied)?;
        if offset.get() != 0 || count.get().checked_mul(PAGE_SIZE) != Some(span.initial_size()) {
            return Err(NestFail::Denied);
        }
        (nest, id)
    };
    nest.release_mapping(id).map_err(cell_fail)?;
    Ok(0)
}

pub(super) fn dispatch(frame: &mut TrapContext, call: NestCall, ident: &Arc<TaskIdent>) {
    let Some(actor) = crate::basis::room::scheduler::slot::current().running_task() else {
        return super::outcome::ret_err(frame, NestFail::Denied);
    };
    if let NestCall::Reserve { token, request } = call {
        match reserve(&actor, token, request, ident) {
            Ok((token, address)) => {
                frame.gpr.set_x(Gprs::A0, token.get());
                frame.gpr.set_x(Gprs::A1, address.get());
            }
            Err(error) => super::outcome::ret_err(frame, error),
        }
        return;
    }
    if let NestCall::Release { token } = call {
        super::outcome::answer(frame, release(&actor, token));
        return;
    }
    super::outcome::answer(
        frame,
        (|| {
            let _serial = actor.pend.lock();
            if !pend::live(&actor) {
                return Err(NestFail::Dead);
            }
            if let NestCall::Delete { token } = call {
                binding::delete(&actor, token)?;
                return Ok(0);
            }
            let _commit = crate::basis::commit();
            match call {
                NestCall::Create { license } => {
                    pend::access::license(&actor, license, CreationRights::NEST)
                        .map_err(pend_fail)?;
                    let nest = crate::basis::unit::team::TeamBuilder::prepare_nest(SpaceKind::User)
                        .map_err(nest_fail)?;
                    let root = Arc::try_new(
                        pend::PendEntry::try_new(
                            pend::Target::Nest {
                                owner: actor.ident.id,
                                nest,
                            },
                            Mark::NONE,
                            Modifiable::Yes,
                            None,
                        )
                        .map_err(pend_fail)?,
                    )
                    .map_err(|_| NestFail::OutOfMemory)?;
                    let token = root.token();
                    actor.pend.prepare(root).map_err(pend_fail)?.publish();
                    Ok(token.get())
                }
                NestCall::Inspect { token, out } => {
                    let info = binding::inspect(&actor, token)?;
                    if !ident.team.nest.copy_out(&info.to_bytes(), out) {
                        return Err(NestFail::Denied);
                    }
                    Ok(0)
                }
                NestCall::Delete { .. } | NestCall::Reserve { .. } | NestCall::Release { .. } => {
                    unreachable!()
                }
            }
        })(),
    );
}
