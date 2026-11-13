//! Fixed-origin partial geometry, admitted by the actual maintenance owner.
use super::*;
use kernel_api::{
    address::{PageCount, PageOffset},
    model::{
        cell::CellLevel,
        flat::{BlockPhase, FlatVersion, Reshape, SlotPosition},
        pend::Operations,
    },
};
pub(super) fn run(
    actor: &Arc<Task>,
    start: SlotPosition,
    expected: FlatVersion,
    parent: CellLevel,
    maintenance: memory::Maintenance,
    direction: crate::basis::flat::Direction,
) -> Result<Value, Failure> {
    let geometry = Reshape::new(start.page(), parent).ok_or(Failure::NotAligned)?;
    let slot = flat::authority::slot(actor, start, memory::rights::RESHAPE).map_err(pend_error)?;
    let target = pend::locate(actor, start.token()).ok_or(Failure::Denied)?;
    let meta = target.target().memory().ok_or(Failure::Denied)?;
    let offset = start
        .page()
        .checked_sub(meta.source().index().get())
        .ok_or(Failure::Denied)?;
    let count = PageCount::new(geometry.count()).ok_or(Failure::NotAligned)?;
    if !target.authority().contains(PageOffset::new(offset), count) {
        return Err(Failure::Denied);
    }
    let selected = slot
        .flat()
        .range(slot.index(), count)
        .map_err(|_| Failure::NotAligned)?;
    let (entry, source, block) = maintenance::resolve(actor, maintenance, true)?;
    if !entry.permits(Operations::Flat(memory::rights::RESHAPE)) || !block.covers(&selected) {
        return Err(Failure::Denied);
    }
    if block.phase() != BlockPhase::Held {
        return Err(Failure::Busy);
    }
    let actual = meta.authorization().ok_or(Failure::Denied)?;
    if !Arc::ptr_eq(&source, &actual) {
        return Err(Failure::Denied);
    }
    source
        .check_move(PageOffset::new(offset), count, Some(entry.authority()))
        .map_err(pend_error)?;
    let _operation = flat::sharing::SharingOperation::acquire(&meta).ok_or(Failure::Busy)?;
    let (old, _) = direction.levels(geometry);
    for head in (0..geometry.count()).step_by(old.pages().unwrap()) {
        let cell = selected
            .cell(PageOffset::new(head))
            .map_err(|_| Failure::NotAligned)?;
        if cell.level() != old {
            return Err(Failure::NotAligned);
        }
        if let Some(origin) = cell.provenance() {
            origin
                .check(actor.ident.id, None, memory::rights::RESHAPE, false)
                .map_err(pend_error)?;
            origin.admit(Some(entry.authority())).map_err(pend_error)?;
        }
    }
    let (version, slots) = block
        .reshape(&selected, geometry, direction, expected)
        .map_err(residency_error)?;
    Ok(Value::Geometry {
        flat: start.flat(),
        version,
        slots,
        range: geometry,
    })
}
