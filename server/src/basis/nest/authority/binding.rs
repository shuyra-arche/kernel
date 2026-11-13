//! A trusted external Nest retains one root authorization and hardware binding.
//! Child Pends govern their own Memo publications; losing a child never closes
//! this address space. The boot root and backend govern full retirement.
use crate::{
    basis::{
        nest::Nest,
        pend::{self, Pend, PendEntry},
        unit::task::Task,
    },
    metal::{iommu::Fault, sync::SpinLock},
};
use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use kernel_api::{
    abi::call::{nest::NestFail, pend::PendFail},
    model::{
        identity::{PendId, TaskId},
        nest::{NestInfo, NestKind, NestPhase},
        pend::{NestRights, NestScope, Operations, PendScope},
    },
};

struct Organization {
    nest: Arc<Nest>,
    root: Weak<Pend>,
    // The same authority, retained only until the complete binding settles.
    // This is an outstanding obligation, not a second authorization record.
    binding: Option<Arc<Pend>>,
    notification: Option<(TaskId, Weak<PendEntry>)>,
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
        .filter(|entry| {
            pend::live(actor)
                && (if rights == NestRights::INSPECT || rights == NestRights::DELETE {
                    entry.authority().permits(Operations::Nest(rights))
                } else {
                    entry.permits(Operations::Nest(rights))
                })
                && entry.target().nest().is_some()
                && entry.authority().scope() == PendScope::Nest(NestScope::Whole)
        })
        .ok_or(NestFail::Denied)
}

/// Trusted boot has already bound the complete, authenticated isolation domain.
/// Register before publishing the root; ordinary Unit creation cannot call this.
pub(crate) fn register(nest: &Arc<Nest>, root: &Arc<Pend>) -> Result<(), PendFail> {
    let mut records = ORGANIZATIONS.lock();
    if !nest.is_external()
        || nest.external_phase() != NestPhase::Bound
        || records.iter().any(|r| Arc::ptr_eq(&r.nest, nest))
    {
        return Err(PendFail::Denied);
    }
    records.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
    root.retain_retirement()?;
    records.push(Organization {
        nest: nest.clone(),
        root: Arc::downgrade(root),
        binding: Some(root.clone()),
        notification: None,
        fault: None,
        fault_sequence: 0,
    });
    Ok(())
}

/// Finish the boot handoff after every fallible ledger preparation succeeded.
/// The holder is a notification destination, not the lifetime of a driver lease.
pub(crate) fn deliver(root: &Arc<PendEntry>, holder: TaskId) {
    let mut records = ORGANIZATIONS.lock();
    let record = records
        .iter_mut()
        .find(|r| r.root.ptr_eq(&Arc::downgrade(root.authority())))
        .expect("registered external Nest root");
    let entry = Arc::downgrade(root);
    assert!(
        record
            .notification
            .as_ref()
            .is_none_or(|(task, known)| *task == holder && known.ptr_eq(&entry))
    );
    record.notification = Some((holder, entry));
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
        info.fault_sequence = record.fault_sequence;
        if let Some(fault) = record.fault {
            info.fault_cause = fault.cause as u64;
            info.fault_address = fault.address;
            info.fault_requester = fault.device as u64;
        }
    }
    Ok(info)
}

pub(crate) fn delete(actor: &Task, token: PendId) -> Result<(), NestFail> {
    let entry = authorized(actor, token, NestRights::DELETE)?;
    let nest = entry.target().nest().ok_or(NestFail::Denied)?;
    if !nest.is_external() {
        if nest.is_adopted() {
            return Err(NestFail::Denied);
        }
        // CPU invalidation must not run under the global commit lock.
        return nest.delete_organization().map_err(|_| NestFail::Busy);
    }
    let _commit = crate::basis::commit();
    {
        let records = ORGANIZATIONS.lock();
        let record = records
            .iter()
            .find(|record| Arc::ptr_eq(&record.nest, &nest))
            .ok_or(NestFail::Denied)?;
        if !record.root.ptr_eq(&Arc::downgrade(entry.authority())) {
            return Err(NestFail::Denied);
        }
    }
    nest.close_external().map_err(|_| NestFail::Denied)?;
    nest.progress_reservations();
    if !super::external::discard(&nest) {
        return Err(NestFail::Busy);
    }
    let mut records = ORGANIZATIONS.lock();
    let at = records
        .iter()
        .position(|record| Arc::ptr_eq(&record.nest, &nest))
        .ok_or(NestFail::Denied)?;
    if records[at].binding.is_some() {
        return Err(NestFail::Busy);
    }
    nest.delete_organization().map_err(|_| NestFail::Busy)?;
    let removed = records.remove(at);
    drop(records);
    drop(removed);
    Ok(())
}

pub(crate) fn maintain() -> bool {
    use core::sync::atomic::{AtomicUsize, Ordering};
    static CURSOR: AtomicUsize = AtomicUsize::new(0);
    for _ in 0..4 {
        let commit = crate::basis::commit();
        let (nest, root, binding) = {
            let Some(records) = ORGANIZATIONS.try_lock() else {
                return true;
            };
            if records.is_empty() {
                break;
            }
            let at = CURSOR.fetch_add(1, Ordering::Relaxed) % records.len();
            let record = &records[at];
            (
                record.nest.clone(),
                record.root.clone(),
                record.binding.is_some(),
            )
        };
        // Inverse references and source loans never leave under the keeper lock.
        let id = nest.id();
        let before = nest.external_phase();
        let invalid = root.upgrade().is_none_or(|root| !root.alive());
        if invalid {
            let _ = nest.close_external();
        }
        let mut faults = [None; 4];
        if binding && before != NestPhase::Closed {
            for slot in &mut faults {
                match nest.external_fault() {
                    Ok(Some(fault)) => *slot = Some(fault),
                    Ok(None) => break,
                    Err(_) => {
                        let _ = nest.close_external();
                        break;
                    }
                }
            }
        }
        nest.progress_reservations();
        let settled = binding && nest.progress_external_close().unwrap_or(false);
        let remove = invalid
            && nest.external_settled()
            && super::external::discard(&nest)
            && nest.delete_organization().is_ok();
        let (retired, observer, removed, fault_changed) = {
            let mut records = ORGANIZATIONS.lock();
            let Some(at) = records
                .iter()
                .position(|record| Arc::ptr_eq(&record.nest, &nest))
            else {
                continue;
            };
            let record = &mut records[at];
            let fault_changed = faults.iter().any(Option::is_some);
            for fault in faults.into_iter().flatten() {
                record.fault.get_or_insert(fault);
                record.fault_sequence = record.fault_sequence.saturating_add(1);
            }
            let retired = if settled { record.binding.take() } else { None };
            let observer = retired.as_ref().and(record.notification.clone());
            let removed = (remove && record.binding.is_none()).then(|| records.remove(at));
            (retired, observer, removed, fault_changed)
        };
        if let Some(root) = retired {
            // SAFETY: the backend proved complete binding retirement, including
            // payloads, queues, tables and trusted platform ordering.
            unsafe { root.release_retirement() };
        }
        let notice =
            observer.and_then(|(task, entry)| entry.upgrade().map(|entry| (task, entry.token())));
        drop(removed);
        drop(commit);
        if let Some((task, token)) = notice {
            pend::notify(task, token);
        }
        if before != nest.external_phase() || fault_changed {
            crate::basis::sign::signal(crate::basis::sign::WakeKey::ExternalNest { id });
        }
    }
    ORGANIZATIONS
        .lock()
        .iter()
        .any(|record| record.binding.is_some())
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
