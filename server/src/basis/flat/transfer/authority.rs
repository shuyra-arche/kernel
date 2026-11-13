//! Holding movement uses the actual caller's two grants, not a peer Task's
//! scheduling state or local handle table. Geometry/holding commits remain in
//! Transaction; publication, maintenance and version policy belongs to its caller.
use super::{Transfer, TransferOutcome};
use crate::basis::{
    flat::{FlatRange, authority::slot},
    nest::Access,
    pend::{self, Geometry, Hold, Pend, SourceAuthorization},
    unit::task::Task,
};
use alloc::sync::Arc;
use kernel_api::{
    abi::call::{cell::CellFail, pend::PendFail},
    address::{PageCount, PageOffset},
    model::{
        flat::SlotPosition,
        identity::PendId,
        pend::{FlatRights, Operations},
    },
};

/// Only Flat is part of the next ABI. Legacy retains an already validated v10
/// private/Share-root window; no SlotId enters this mechanism.
pub(crate) enum Endpoint {
    Flat(SlotPosition),
    Legacy {
        hold: Arc<Hold>,
        relative: PageOffset,
    },
}

struct Selection {
    range: FlatRange,
    source: Arc<SourceAuthorization>,
    branch: Option<(PendId, Arc<Pend>)>,
    offset: PageOffset,
}
impl Endpoint {
    /// Normalize a v10 handle exactly once, preserving its narrower immutable
    /// window. A Flat endpoint thereafter resolves by Pend + fixed coordinates.
    pub(crate) fn legacy(
        actor: &Arc<Task>,
        hold: Arc<Hold>,
        relative: PageOffset,
    ) -> Result<Self, CellFail> {
        let range = hold.resolve().ok_or(CellFail::Denied)?.cell(relative)?;
        if let Some((token, authority)) = hold.branch()
            && matches!(authority.limits().operations, Operations::Flat(_))
        {
            return SlotPosition::new(token, range.source_id(), range.index().get())
                .map(Self::Flat)
                .ok_or(CellFail::NotAligned);
        }
        if !hold.source().manages(actor) {
            return Err(CellFail::Denied);
        }
        Ok(Self::Legacy { hold, relative })
    }

    fn resolve(&self, actor: &Arc<Task>) -> Result<Selection, CellFail> {
        let selected = match self {
            Self::Flat(position) => {
                let range = slot(actor, *position, FlatRights::TRANSFER).map_err(fail)?;
                let entry = pend::locate(actor, position.token()).ok_or(CellFail::Denied)?;
                let meta = entry.target().memory().ok_or(CellFail::Denied)?;
                let source = meta.authorization().ok_or(CellFail::Denied)?;
                let offset = position
                    .page()
                    .checked_sub(meta.source().index().get())
                    .ok_or(CellFail::Denied)?;
                Selection {
                    range,
                    source,
                    branch: Some((position.token(), entry.authority().clone())),
                    offset: PageOffset::new(offset),
                }
            }
            Self::Legacy { hold, relative } => {
                if !hold.source().manages(actor) {
                    return Err(CellFail::Denied);
                }
                if let Some((token, _)) = hold.branch()
                    && !hold
                        .source()
                        .root()
                        .is_some_and(|(_, root, _)| token == root)
                {
                    return Err(CellFail::Denied);
                }
                let range = hold.resolve().ok_or(CellFail::Denied)?.cell(*relative)?;
                let offset = hold
                    .window()
                    .0
                    .get()
                    .checked_add(relative.get())
                    .ok_or(CellFail::NotAligned)?;
                Selection {
                    range,
                    source: hold.source().clone(),
                    branch: hold.branch().map(|(t, p)| (t, p.clone())),
                    offset: PageOffset::new(offset),
                }
            }
        };
        if !selected.source.branch_check(
            selected.branch.as_ref().map(|(_, p)| p.as_ref()),
            Access::READ | Access::WRITE,
        ) {
            return Err(CellFail::Denied);
        }
        Ok(selected)
    }
}

/// Caller owns its ledger serial for the complete operation. No peer Task is
/// consulted. Both endpoints must pass before either admission flight is added.
pub(crate) fn execute(
    actor: &Arc<Task>,
    from: Endpoint,
    to: Endpoint,
) -> Result<TransferOutcome, CellFail> {
    let (source, target, page) = {
        let _commit = crate::basis::commit();
        if !pend::live(actor) {
            return Err(CellFail::Denied);
        }
        let source = from.resolve(actor)?;
        let target = to.resolve(actor)?;
        let page = source
            .range
            .page(PageOffset::ZERO)?
            .ok_or(CellFail::SourceEmpty)?;
        if target.range.level() != page.level() {
            return Err(CellFail::NotAligned);
        }
        (source, target, page)
    };
    let transfer = Transfer::new(
        source.range.clone(),
        PageOffset::ZERO,
        target.range.clone(),
        PageOffset::ZERO,
    )?;
    let permit = crate::basis::cell::change::find(&page);
    let flight = next_flight()?;
    let count = PageCount::new(page.pages()).ok_or(CellFail::NotAligned)?;
    let windows = [
        Geometry::new(source.offset, count).map_err(fail)?,
        Geometry::new(target.offset, count).map_err(fail)?,
    ];
    let selected = [source, target];
    {
        let _commit = crate::basis::commit();
        if !pend::live(actor) {
            return Err(CellFail::Denied);
        }
        for (endpoint, old) in [&from, &to].into_iter().zip(&selected) {
            let current = endpoint.resolve(actor)?;
            if !current.range.same_range(&old.range) || !Arc::ptr_eq(&current.source, &old.source) {
                return Err(CellFail::Denied);
            }
            current
                .source
                .check_move(
                    current.offset,
                    count,
                    current.branch.as_ref().map(|(_, p)| p.as_ref()),
                )
                .map_err(fail)?;
        }
        if !selected[0]
            .range
            .page(PageOffset::ZERO)?
            .is_some_and(|current| Arc::ptr_eq(&current, &page))
        {
            return Err(CellFail::Busy);
        }
        for end in &selected {
            end.source
                .retain_blocked(end.offset, count, &page)
                .map_err(fail)?;
        }
        for (at, end) in selected.iter().enumerate() {
            if let Err(error) = end.source.enter_publish_range(
                flight,
                windows[at].clone(),
                end.branch.as_ref().map(|(_, p)| p.as_ref()),
            ) {
                for entered in &selected[..at] {
                    entered.source.leave_publish(flight);
                }
                return Err(fail(error));
            }
        }
    }
    let outcome = transfer.execute(permit.as_ref());
    for end in &selected {
        end.source.leave_publish(flight);
    }
    Ok(outcome)
}

fn next_flight() -> Result<usize, CellFail> {
    static NEXT: crate::basis::name::Serial =
        crate::basis::name::Serial::new(1, isize::MAX as usize);
    NEXT.allocate()
        .map(|id| (1 << (usize::BITS - 1)) | id)
        .ok_or(CellFail::OutOfMemory)
}
fn fail(error: PendFail) -> CellFail {
    match error {
        PendFail::OutOfMemory => CellFail::OutOfMemory,
        PendFail::Busy => CellFail::Busy,
        PendFail::NotAligned => CellFail::NotAligned,
        _ => CellFail::Denied,
    }
}
