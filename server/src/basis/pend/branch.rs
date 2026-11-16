use alloc::sync::Arc;
use alloc::sync::Weak;

use kernel_api::abi::call::pend::PendFail;
use kernel_api::model::identity::Mark;
use kernel_api::model::identity::PendId;

use super::PendEntry;
use super::ledger::pair;
use crate::basis::flat::source::Operation;
use crate::basis::sign;
use crate::basis::sign::WakeKey;
use crate::basis::unit::task::Task;
use kernel_api::address::PageCount;
use kernel_api::address::PageOffset;
use kernel_api::model::pend::{Limits, NestScope, PendRange, PendScope};

/// Caller holds the ledger serial through normalization and the authority effect.
fn effective_limits(
    entry: &PendEntry,
    limits: Limits,
    requested: super::Window,
) -> Result<Limits, PendFail> {
    if !limits.valid() {
        return Err(PendFail::Denied);
    }
    let scope = requested.scope(entry.scope(), limits.scope)?;
    if let Some(PendRange::Pages { offset, count }) = scope.range() {
        let offset = PageOffset::new(offset);
        let count = PageCount::new(count).ok_or(PendFail::Denied)?;
        match scope {
            PendScope::Pages(_) => {
                entry
                    .target()
                    .memory()
                    .ok_or(PendFail::Denied)?
                    .source()
                    .range(offset, count)
                    .map_err(|_| PendFail::NotAligned)?;
            }
            PendScope::Nest(NestScope::Reservation { id, .. }) => {
                let nest = entry.target().nest().ok_or(PendFail::Denied)?;
                if !nest.reservation_open(id) {
                    return Err(PendFail::Dead);
                }
                let span = nest.reservation(id).ok_or(PendFail::Denied)?;
                span.range(offset, count)
                    .map_err(|_| PendFail::NotAligned)?;
                if nest.is_external()
                    && (offset != PageOffset::ZERO || Some(count) != nest.reservation_count(id))
                {
                    return Err(PendFail::NotAligned);
                }
            }
            _ => return Err(PendFail::Denied),
        }
    }
    Ok(Limits { scope, ..limits })
}

/// Local derivation and cross-Task delegation share the same authorization commit.
pub(crate) fn accord(
    caller: &Arc<Task>,
    src: PendId,
    dst: &Weak<Task>,
    limits: Limits,
    mark: Mark,
    requested: super::Window,
    permit: Option<(PendId, kernel_api::model::pend::Operations)>,
) -> Result<usize, PendFail> {
    let share = permit.is_some_and(|(_, operation)| {
        operation
            == kernel_api::model::pend::Operations::None(
                kernel_api::model::pend::CreationRights::SHARE,
            )
    }) && matches!(
        limits.operations,
        kernel_api::model::pend::Operations::Share(_)
    );
    let target = dst.upgrade().ok_or(PendFail::Denied)?;
    let observed = super::locate(caller, src).ok_or(PendFail::Denied)?;
    if !observed.derives(limits, share) {
        return Err(PendFail::Denied);
    }
    let share_view = observed.target().memory();
    let _operation = if let Some(meta) = &share_view {
        let operation = Operation::acquire(meta).ok_or(PendFail::Busy)?;
        Some(operation)
    } else {
        None
    };
    // A device register window requires exclusive writable custody; this
    // differs from Page-backed Share, whose CPU mappings may coexist.
    let registers = share_view
        .as_ref()
        .filter(|meta| meta.source().is_borrowed());
    let check_source = |window: Option<(PageOffset, PageCount)>| {
        if let Some(meta) = &share_view
            && let Some(authorization) = meta.authorization()
        {
            let (offset, count) = window.unwrap_or((PageOffset::ZERO, meta.source().page_count()));
            authorization.check_publish(offset, count)?;
        }
        Ok::<(), PendFail>(())
    };
    let (pend_entry, granted, heir, registration, limits, window, withdraw_registers) = {
        let _pair = pair(caller, &target)?;
        let _commit = crate::basis::commit();
        if !super::live(caller) || !super::live(&target) {
            return Err(PendFail::Dead);
        }
        if let Some((token, operation)) = permit {
            if !super::locate(caller, token).is_some_and(|entry| entry.permits(operation)) {
                return Err(PendFail::Denied);
            }
        }
        let pend_entry = super::locate(caller, src).ok_or(PendFail::Denied)?;
        if !Arc::ptr_eq(&pend_entry, &observed) || !pend_entry.alive() {
            return Err(PendFail::Dead);
        }
        if !pend_entry.derives(limits, share) {
            return Err(PendFail::Denied);
        }
        let mark = if mark == Mark::NONE {
            pend_entry.mark()
        } else {
            mark
        };
        if registers.is_some() {
            pend_entry
                .authority()
                .can_lend_registers(limits.modifiable)?;
        } else if share_view.is_some() && pend_entry.authority().modifiable().is_none() {
            return Err(PendFail::Busy);
        }
        let limits = effective_limits(&pend_entry, limits, requested)?;
        let window = match limits.scope.range() {
            Some(PendRange::Pages { offset, count }) => Some((
                PageOffset::new(offset),
                PageCount::new(count).ok_or(PendFail::Denied)?,
            )),
            None => None,
            _ => return Err(PendFail::Denied),
        };
        check_source(window)?;
        let mut granted = pend_entry.vest_scope(mark, limits, share)?;
        granted.parent(src, Arc::downgrade(caller));
        let granted = Arc::try_new(granted).map_err(|_| PendFail::OutOfMemory)?;
        let heir = caller
            .pend
            .prepare_heir(src, Arc::downgrade(&target), granted.token())?;
        let registration = target.pend.prepare(granted.clone())?;
        let withdraw_registers = registers.is_some()
            && pend_entry.modifiable() == kernel_api::model::modifiable::Modifiable::Yes;
        (
            pend_entry,
            granted,
            heir,
            registration,
            limits,
            window,
            withdraw_registers,
        )
    };
    // These are owned, unpublished registration/return records, not software
    // guards. Ledger closure may proceed while the exact old mappings drain;
    // the operation lease and records retain every rollback obligation.
    if withdraw_registers {
        crate::basis::room::interrupt::withdraw(src, pend_entry.target());
        registers
            .expect("register withdrawal source")
            .close_maps(Some(src))?;
    }
    let token = {
        let _pair = pair(caller, &target)?;
        let _commit = crate::basis::commit();
        if !super::live(caller) || !super::live(&target) {
            return Err(PendFail::Dead);
        }
        let current = super::locate(caller, src).ok_or(PendFail::Denied)?;
        if !Arc::ptr_eq(&current, &pend_entry)
            || !current.alive()
            || !current.derives(limits, share)
            || permit.is_some_and(|(token, operation)| {
                !super::locate(caller, token).is_some_and(|e| e.permits(operation))
            })
            || window.is_some_and(|(offset, count)| !current.authority().contains(offset, count))
        {
            return Err(PendFail::Denied);
        }
        check_source(window)?;
        if registers.is_some() {
            current.authority().lend_registers(granted.authority())?;
        }
        let token = granted.token();
        crate::basis::room::interrupt::dispatch(&target, &granted)?;
        heir.publish();
        registration.publish();
        token
    };
    let _ = sign::wake(
        WakeKey::Pends {
            task: target.ident.id,
        },
        &target.life(),
    );
    super::notify(caller.ident.id, src);
    super::notify(target.ident.id, token);
    Ok(token.get())
}

/// Narrow preserves identity. Live descendants/external loans return Busy before
/// changing the window; the holder can Return/Revoke them and retry.
pub(crate) fn reduce(task: &Arc<Task>, token: PendId, limits: Limits) -> Result<(), PendFail> {
    {
        let observed = super::locate(task, token).ok_or(PendFail::Denied)?;
        let share = observed.target().memory();
        let _operation = share
            .as_ref()
            .map(|meta| Operation::acquire(meta).ok_or(PendFail::Busy))
            .transpose()?;
        let (observed, limits, authorization) = {
            let _serial = task.pend.lock();
            let observed = super::locate(task, token).ok_or(PendFail::Denied)?;
            if !super::live(task) || !observed.alive() {
                return Err(PendFail::Dead);
            }
            let limits = effective_limits(
                &observed,
                limits,
                super::Window::Parent(limits.scope.range().unwrap_or(PendRange::Inherit)),
            )?;
            observed.authority().can_narrow(limits)?;
            if task
                .pend
                .heirs
                .lock()
                .iter()
                .any(|(parent, _, _)| *parent == token)
            {
                return Err(PendFail::Busy);
            }
            let authorization = share.as_ref().and_then(|meta| meta.authorization());
            if authorization
                .as_ref()
                .is_some_and(|source| source.blocked_by(observed.authority()))
            {
                return Err(PendFail::Busy);
            }
            {
                let _commit = crate::basis::commit();
                let _withdraw = authorization
                    .as_ref()
                    .map(|source| source.begin_withdraw())
                    .transpose()?;
            }
            (observed, limits, authorization)
        };
        // Persistent publisher routes discharge only through exact hardware withdrawal.
        if let PendScope::Nest(NestScope::Reservation { id, .. }) = observed.scope() {
            let window = observed.authority().window().ok_or(PendFail::Denied)?;
            observed
                .target()
                .nest()
                .ok_or(PendFail::Denied)?
                .withdraw_reservation(id, window, observed.authority())?;
        }
        if let Some(meta) = &share {
            meta.close_maps(Some(token))?;
            crate::basis::nest::authority::withdraw_source_branch(meta, token, false)?;
        }
        {
            let _serial = task.pend.lock();
            let _commit = crate::basis::commit();
            let current = super::locate(task, token).ok_or(PendFail::Denied)?;
            if !super::live(task) || !Arc::ptr_eq(&current, &observed) {
                return Err(PendFail::Dead);
            }
            // Delegation may have occurred while hardware withdrawal ran. Its
            // durable heirs must be checked again before committing the narrower
            // authority; withdrawn routes alone never grant permission to narrow.
            if task
                .pend
                .heirs
                .lock()
                .iter()
                .any(|(parent, _, _)| *parent == token)
                || authorization
                    .as_ref()
                    .is_some_and(|source| source.blocked_by(current.authority()))
            {
                return Err(PendFail::Busy);
            }
            let _withdraw = authorization
                .as_ref()
                .map(|source| source.begin_withdraw())
                .transpose()?;
            current.narrow(limits)?;
        }
    }
    super::notify(task.ident.id, token);
    Ok(())
}
