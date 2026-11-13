//! v10 Slot registration and geometry adapters. Native holding movement lives
//! under basis::flat::transfer and has no Task-local handle arguments.
use super::super::memory::{caller, source_fail, wake};
use crate::basis::{
    flat::{Flat, FlatRange},
    pend::{self, SourceAuthorization},
    unit::task::{Task, TaskIdent},
};
use alloc::sync::Arc;
use kernel_api::{
    abi::call::cell::CellFail,
    address::{PageOffset, VirtAddr as KVirt},
    model::{cell::*, identity::PendId},
};
mod shape;
pub(in crate::entry::call) use shape::reshape;
/// Legacy v10 creation returns a SlotId; Pend-backed association does not.
/// Caller holds the Task ledger serial and COMMIT until the reference is stored.
pub(in crate::entry::call) fn create(
    task: &Arc<Task>,
    license: PendId,
    allocation: kernel_api::model::cell::Allocation,
) -> Result<usize, CellFail> {
    let (range, authorization) =
        crate::basis::flat::authority::allocate(task, license, allocation).map_err(source_fail)?;
    let ceiling = authorization.ceiling();
    task.pend
        .references
        .insert(
            authorization,
            PageOffset::ZERO,
            range.page_count(),
            ceiling,
            None,
        )
        .map_err(source_fail)
}

pub(in crate::entry::call) fn release(
    reference: kernel_api::model::cell::SlotId,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    let task = caller(ident).ok_or(CellFail::Denied)?;
    let _serial = task.pend.lock();
    if !pend::live(&task) {
        return Err(CellFail::Denied);
    }
    let hold = task
        .pend
        .references
        .remove(reference.get())
        .ok_or(CellFail::Denied)?;
    drop(hold);
    Ok(0)
}

pub(in crate::entry::call) fn transfer(
    from: kernel_api::model::cell::SlotId,
    from_page: usize,
    to: kernel_api::model::cell::SlotId,
    to_page: usize,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    use crate::basis::flat::transfer::authority::{self, Endpoint};
    let actor = caller(ident).ok_or(CellFail::Denied)?;
    let serial = actor.pend.lock();
    let (source, target) = {
        let _commit = crate::basis::commit();
        if !pend::live(&actor) {
            return Err(CellFail::Denied);
        }
        let source = actor
            .pend
            .references
            .get(from.get())
            .ok_or(CellFail::Denied)?;
        let target = actor
            .pend
            .references
            .get(to.get())
            .ok_or(CellFail::Denied)?;
        (
            Endpoint::legacy(&actor, source, PageOffset::new(from_page))?,
            Endpoint::legacy(&actor, target, PageOffset::new(to_page))?,
        )
    };
    let outcome = authority::execute(&actor, source, target)?;
    drop(serial);
    wake(outcome.affected);
    outcome.result.map(|()| 0).map_err(CellFail::from)
}

fn endpoint(
    caller: &Arc<Task>,
    reference: kernel_api::model::cell::SlotId,
) -> Result<FlatRange, CellFail> {
    let hold = caller
        .pend
        .references
        .get(reference.get())
        .ok_or(CellFail::Denied)?;
    let flat = hold.authority().is_some_and(|p| {
        matches!(
            p.limits().operations,
            kernel_api::model::pend::Operations::Flat(_)
        )
    });
    if !flat && !hold.source().manages(caller) {
        return Err(CellFail::Denied);
    }
    hold.resolve().ok_or(CellFail::Denied)
}

pub(in crate::entry::call) fn inspect(
    pos: CellPosition,
    ident: &Arc<TaskIdent>,
) -> Result<CellInfo, CellFail> {
    let CellObject::Slot(slot) = pos.object else {
        return Err(CellFail::Denied);
    };
    let actor = caller(ident).ok_or(CellFail::Denied)?;
    let _serial = actor.pend.lock();
    let range = endpoint(&actor, slot)?.cell(PageOffset::new(pos.page))?;
    let page = range
        .materialized_cells()?
        .into_iter()
        .next()
        .map(|(_, page)| page);
    Ok(CellInfo {
        level: range.level(),
        generation: 0,
        state: if page.is_some() {
            CellState::Ready
        } else {
            CellState::Vacant
        },
        page: page.as_ref().map_or(0, |page| page.id().get() as u64),
        frozen: page.as_ref().is_some_and(|page| page.frozen()),
        external: page
            .as_ref()
            .is_some_and(|page| page.exposure() == crate::basis::cell::page::Exposure::External),
        address: KVirt::new(0),
        fault_cause: 0,
        fault_address: 0,
    })
}
