//! External mapping observations use the Nest's authoritative span and Memo state.
use crate::basis::nest::Nest;
use crate::basis::pend::{self, PendEntry};
use crate::basis::unit::{anchor::Anchor, task::Task};
use alloc::sync::{Arc, Weak};
use kernel_api::abi::call::cell::CellFail;
use kernel_api::model::cell::{CellState, MappingId};
use kernel_api::model::identity::PendId;
use kernel_api::model::pend::{NestRights, NestScope, Operations, PendScope};

fn authorized(actor: &Task, token: PendId, id: MappingId) -> Result<Arc<PendEntry>, CellFail> {
    let entry = pend::locate(actor, token).ok_or(CellFail::Denied)?;
    let nest = entry.target().nest().ok_or(CellFail::Denied)?;
    if !pend::live(actor)
        || !entry
            .authority()
            .permits(Operations::Nest(NestRights::INSPECT))
        || !matches!(entry.authority().scope(), PendScope::Nest(NestScope::Mapping { id: selected, .. }) if selected == id)
        || !nest.is_external()
    {
        return Err(CellFail::Denied);
    }
    let span = nest.mapping(id).ok_or(CellFail::Denied)?;
    let count = kernel_api::address::PageCount::new(span.grain()).ok_or(CellFail::Denied)?;
    if !entry
        .authority()
        .contains(kernel_api::address::PageOffset::ZERO, count)
    {
        return Err(CellFail::Denied);
    }
    Ok(entry)
}

pub(crate) fn event(
    actor: &Task,
    token: PendId,
    id: MappingId,
    published: bool,
    expected: u64,
) -> Result<
    (
        Weak<Anchor>,
        Option<Option<kernel_api::abi::call::sign::SignEventFail>>,
    ),
    CellFail,
> {
    use kernel_api::abi::call::sign::SignEventFail;
    let entry = authorized(actor, token, id)?;
    let nest = entry.target().nest().ok_or(CellFail::Denied)?;
    let span = nest.mapping(id).ok_or(CellFail::Denied)?;
    let info = span.inspect_memo(0)?;
    if expected == 0 {
        return Err(CellFail::Denied);
    }
    let ready = if info.generation > expected {
        Some(Some(SignEventFail::Gone))
    } else if info.generation < expected {
        let next = info.generation.checked_add(1) == Some(expected)
            && if published {
                info.state == CellState::Ready
            } else {
                matches!(info.state, CellState::Published | CellState::Publishing)
            };
        if next {
            None
        } else {
            Some(Some(SignEventFail::Gone))
        }
    } else if info.state == CellState::Failed || nest.external_error().is_some() {
        Some(Some(SignEventFail::Busy))
    } else if if published {
        info.state == CellState::Published
    } else {
        matches!(info.state, CellState::Ready | CellState::Vacant)
    } {
        Some(None)
    } else {
        None
    };
    Ok((nest.life(), ready))
}

pub(crate) fn maintain() -> bool {
    crate::basis::cell::change::maintain();
    if let Some(route) = crate::basis::room::interrupt::external() {
        route.rearm();
    }
    super::binding::maintain()
}

/// Gone metadata is not an occupied address or an unsettled device use.
pub(super) fn discard(nest: &Arc<Nest>) -> bool {
    nest.mappings_gone()
}
