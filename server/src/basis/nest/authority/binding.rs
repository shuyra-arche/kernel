//! Retained authorization for a Nest's exclusive source binding. Translation,
//! Page and Memo state stay in Nest; this registry retains the responsible Pends.
use crate::{
    basis::{
        nest::Nest,
        pend::{self, PendEntry},
        unit::task::Task,
    },
    metal::{iommu::Fault, sync::SpinLock},
};
use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use kernel_api::{
    abi::call::nest::NestFail,
    model::{
        identity::PendId,
        nest::{NestInfo, NestKind, NestPhase},
        pend::{NestRights, Operations, PortRights},
    },
};
struct Lease {
    actor: Weak<Task>,
    nest: Arc<PendEntry>,
    port: Arc<PendEntry>,
}
struct Organization {
    nest: Arc<Nest>,
    owner: Weak<Task>,
    root: Weak<PendEntry>,
    lease: Option<Lease>,
    port: usize,
    fault: Option<Fault>,
    fault_sequence: u64,
}
static ORGANIZATIONS: SpinLock<Vec<Organization>> = SpinLock::new(Vec::new());
pub(crate) fn authorized(
    actor: &Task,
    token: PendId,
    rights: NestRights,
) -> Result<Arc<PendEntry>, NestFail> {
    pend::locate(actor, token)
        .filter(|e| e.permits(Operations::Nest(rights)) && e.target().nest().is_some())
        .ok_or(NestFail::Denied)
}
pub(crate) fn register(
    nest: &Arc<Nest>,
    owner: &Arc<Task>,
    root: &Arc<PendEntry>,
) -> Result<(), NestFail> {
    let mut records = ORGANIZATIONS.lock();
    records.try_reserve(1).map_err(|_| NestFail::OutOfMemory)?;
    records.push(Organization {
        nest: nest.clone(),
        owner: Arc::downgrade(owner),
        root: Arc::downgrade(root),
        lease: None,
        port: 0,
        fault: None,
        fault_sequence: 0,
    });
    Ok(())
}
pub(crate) fn attach(actor: &Arc<Task>, token: PendId, port: PendId) -> Result<(), NestFail> {
    let nest = authorized(actor, token, NestRights::ATTACH)?;
    let port = pend::locate(actor, port)
        .filter(|e| e.permits(Operations::Port(PortRights::ATTACH)))
        .ok_or(NestFail::Denied)?;
    let source = port.target().port().ok_or(NestFail::Denied)?;
    let target = nest.target().nest().ok_or(NestFail::Denied)?;
    if !target.is_external() || actor.ident.team.external.closed() {
        return Err(NestFail::Denied);
    }
    let mut records = ORGANIZATIONS.lock();
    let record = records
        .iter_mut()
        .find(|r| Arc::ptr_eq(&r.nest, &target))
        .ok_or(NestFail::Denied)?;
    if record.lease.is_some() || target.external_phase() != NestPhase::Unbound {
        return Err(NestFail::Busy);
    }
    nest.authority()
        .retain_route()
        .map_err(|_| NestFail::Busy)?;
    if port.authority().retain_route().is_err() {
        unsafe { nest.authority().release_route() };
        return Err(NestFail::Busy);
    }
    let hardware = match source.bind() {
        Ok(hardware) => hardware,
        Err(_) => {
            unsafe {
                nest.authority().release_route();
                port.authority().release_route();
            }
            return Err(NestFail::Busy);
        }
    };
    // The same COMMIT transaction prevents another binding or close here.
    if let Err((_, hardware)) = target.bind_external(hardware) {
        drop(hardware); // failed backend Drop retains its hardware obligations
        unsafe {
            nest.authority().release_route();
            port.authority().release_route();
        }
        return Err(NestFail::Busy);
    }
    record.port = source.id().get();
    record.lease = Some(Lease {
        actor: Arc::downgrade(actor),
        nest,
        port,
    });
    Ok(())
}
pub(crate) fn inspect(actor: &Task, token: PendId) -> Result<NestInfo, NestFail> {
    let entry = authorized(actor, token, NestRights::INSPECT)?;
    let nest = entry.target().nest().ok_or(NestFail::Denied)?;
    let mut info = NestInfo {
        id: nest.id(),
        kind: if nest.is_external() {
            NestKind::External
        } else {
            NestKind::Internal
        },
        phase: nest.external_phase(),
        port: 0,
        fault_sequence: 0,
        fault_cause: 0,
        fault_address: 0,
        fault_requester: 0,
    };
    if nest.is_external() {
        let records = ORGANIZATIONS.lock();
        let record = records
            .iter()
            .find(|r| Arc::ptr_eq(&r.nest, &nest))
            .ok_or(NestFail::Denied)?;
        info.port = record.port;
        info.fault_sequence = record.fault_sequence;
        if let Some(fault) = record.fault {
            info.fault_cause = fault.cause as u64;
            info.fault_address = fault.address;
            info.fault_requester = fault.device as u64;
        }
    }
    Ok(info)
}
pub(crate) fn detach(actor: &Task, token: PendId) -> Result<(), NestFail> {
    let nest = authorized(actor, token, NestRights::DETACH)?
        .target()
        .nest()
        .ok_or(NestFail::Denied)?;
    nest.close_external().map_err(|_| NestFail::Denied)
}
pub(crate) fn delete(actor: &Task, token: PendId) -> Result<(), NestFail> {
    let nest = authorized(actor, token, NestRights::DELETE)?
        .target()
        .nest()
        .ok_or(NestFail::Denied)?;
    let mut records = ORGANIZATIONS.lock();
    if records
        .iter()
        .any(|r| Arc::ptr_eq(&r.nest, &nest) && r.lease.is_some())
    {
        return Err(NestFail::Busy);
    }
    nest.delete_organization().map_err(|_| NestFail::Busy)?;
    records.retain(|r| !Arc::ptr_eq(&r.nest, &nest));
    Ok(())
}
pub(crate) fn maintain() -> bool {
    use core::sync::atomic::{AtomicUsize, Ordering};
    static CURSOR: AtomicUsize = AtomicUsize::new(0);
    for _ in 0..4 {
        let mut notices = None;
        let changed;
        let _commit = crate::basis::commit();
        let Some(mut records) = ORGANIZATIONS.try_lock() else {
            return true;
        };
        if records.is_empty() {
            break;
        }
        let at = CURSOR.fetch_add(1, Ordering::Relaxed) % records.len();
        let record = &mut records[at];
        let before = (record.nest.external_phase(), record.fault_sequence);
        let id = record.nest.id();
        let invalid = record.owner.upgrade().is_none_or(|a| a.dead())
            || record.root.upgrade().is_none()
            || record.lease.as_ref().is_some_and(|lease| {
                lease
                    .actor
                    .upgrade()
                    .is_none_or(|a| a.dead() || a.ident.team.external.closed())
                    || !lease.nest.permits(Operations::Nest(NestRights::ATTACH))
                    || !lease.port.permits(Operations::Port(PortRights::ATTACH))
            });
        if invalid {
            let _ = record.nest.close_external();
        }
        if record.lease.is_some() {
            for _ in 0..4 {
                match record.nest.external_fault() {
                    Ok(Some(fault)) => {
                        // Hardware records have no software generation. Retain at
                        // binding scope; never stamp a new Memo with an old fault.
                        record.fault.get_or_insert(fault);
                        record.fault_sequence = record.fault_sequence.saturating_add(1);
                    }
                    Ok(None) => break,
                    Err(_) => {
                        let _ = record.nest.close_external();
                        break;
                    }
                }
            }
            if record.nest.progress_external_close().unwrap_or(false) {
                if let Some(lease) = record.lease.take() {
                    unsafe {
                        lease.nest.authority().release_route();
                        lease.port.authority().release_route();
                    }
                    notices = lease
                        .actor
                        .upgrade()
                        .map(|actor| (actor.ident.id, lease.nest.token(), lease.port.token()));
                }
            }
        }
        changed = before != (record.nest.external_phase(), record.fault_sequence);
        if record.lease.is_none()
            && record.root.upgrade().is_none()
            && record.nest.external_settled()
            && super::external::discard(&record.nest)
            && record.nest.delete_organization().is_ok()
        {
            records.remove(at);
        }
        drop(records);
        drop(_commit);
        if let Some((task, nest, port)) = notices {
            pend::notify(task, nest);
            pend::notify(task, port);
        }
        if changed {
            crate::basis::sign::signal(crate::basis::sign::WakeKey::ExternalNest { id });
        }
    }
    ORGANIZATIONS.lock().iter().any(|r| r.lease.is_some())
}

pub(crate) fn event(
    actor: &Task,
    token: PendId,
    fault: bool,
) -> Result<
    (
        Weak<crate::basis::unit::anchor::Anchor>,
        Option<Option<kernel_api::abi::call::sign::SignEventFail>>,
    ),
    NestFail,
> {
    use kernel_api::abi::call::sign::SignEventFail;
    let info = inspect(actor, token)?;
    if info.kind != NestKind::External {
        return Err(NestFail::Denied);
    }
    let nest = authorized(actor, token, NestRights::INSPECT)?
        .target()
        .nest()
        .ok_or(NestFail::Denied)?;
    let result = if fault && info.fault_sequence != 0 {
        Some(None)
    } else if info.phase == NestPhase::Failed {
        Some(Some(SignEventFail::Busy))
    } else if !fault && info.phase == NestPhase::Closed {
        Some(None)
    } else {
        None
    };
    Ok((nest.life(), result))
}
