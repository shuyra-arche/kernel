//! ABI v10 compatibility only. All resource operations belong to Flat/Nest.
//! The next memory call surface has no Cell domain or SlotId descriptors.
use super::{
    flat::legacy as flat,
    memory::{caller, read_snapshot},
    nest::memo as nest,
};
use crate::{basis::unit::task::TaskIdent, metal::switcher::context::TrapContext};
use alloc::sync::Arc;
use kernel_api::{
    abi::call::cell::{CellCall, CellFail},
    address::VirtAddr as KVirt,
    model::cell::*,
};

pub(super) fn dispatch(frame: &mut TrapContext, call: CellCall, ident: &Arc<TaskIdent>) {
    super::outcome::answer(frame, execute(call, ident));
}
fn position(at: KVirt, ident: &Arc<TaskIdent>) -> Result<CellPosition, CellFail> {
    CellPosition::from_bytes(&read_snapshot::<{ CellPosition::WIDTH }>(
        &ident.team.nest,
        at,
    )?)
    .ok_or(CellFail::NotAligned)
}

fn execute(call: CellCall, ident: &Arc<TaskIdent>) -> Result<usize, CellFail> {
    match call {
        CellCall::Create {
            license,
            allocation,
        } => {
            let actor = caller(ident).ok_or(CellFail::Denied)?;
            let _serial = actor.pend.lock();
            let _commit = crate::basis::commit();
            flat::create(&actor, license, allocation)
        }
        CellCall::Delete { slot } => flat::release(slot, ident),
        CellCall::Split { slot } => flat::reshape(slot, false, ident),
        CellCall::Merge { slot } => flat::reshape(slot, true, ident),
        CellCall::Push { request } | CellCall::Pull { request } => {
            let spec = CellHandoff::from_bytes(&read_snapshot::<{ CellHandoff::WIDTH }>(
                &ident.team.nest,
                request,
            )?)
            .ok_or(CellFail::NotAligned)?;
            let (source, target) = if matches!(call, CellCall::Push { .. }) {
                (spec.local, spec.remote)
            } else {
                (spec.remote, spec.local)
            };
            let (CellObject::Slot(from), CellObject::Slot(to)) = (source.object, target.object)
            else {
                return Err(CellFail::Denied);
            };
            flat::transfer(from, source.page, to, target.page, ident)
        }
        CellCall::Insert { request } => nest::associate(
            CellAssociation::from_bytes(&read_snapshot::<{ CellAssociation::WIDTH }>(
                &ident.team.nest,
                request,
            )?)
            .ok_or(CellFail::NotAligned)?,
            ident,
        ),
        CellCall::Publish { position: at } => nest::publication(position(at, ident)?, true, ident),
        CellCall::Retract { position: at } => nest::publication(position(at, ident)?, false, ident),
        CellCall::Inspect { position: at, out } => {
            let pos = position(at, ident)?;
            let info = match pos.object {
                CellObject::Slot(_) => flat::inspect(pos, ident)?,
                CellObject::Pend(_) => nest::inspect(pos, ident)?,
            };
            if !ident.team.nest.copy_out(&info.to_bytes(), out) {
                return Err(CellFail::Denied);
            }
            Ok(0)
        }
        CellCall::Remove { position: at } => nest::remove(position(at, ident)?, ident),
        CellCall::Protect {
            position: at,
            access,
        } => nest::protect(position(at, ident)?, access, ident),
    }
}
