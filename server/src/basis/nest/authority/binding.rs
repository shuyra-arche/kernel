//! A trusted external Nest retains one root authorization and hardware binding.
//! Child Pends govern their own Memo publications; losing a child never closes
//! this address space. Only the root, Delete, or a backend failure can do that.
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
        pend::{NestRights, Operations},
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
        .filter(|e| e.permits(Operations::Nest(rights)) && e.target().nest().is_some())
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
    assert!(record
        .notification
        .as_ref()
        .is_none_or(|(task, known)| *task == holder && known.ptr_eq(&entry)));
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
    let nest = authorized(actor, token, NestRights::DELETE)?
        .target()
        .nest()
        .ok_or(NestFail::Denied)?;
    if !nest.is_external() {
        // CPU invalidation must not run under the global commit lock.
        return nest.delete_organization().map_err(|_| NestFail::Busy);
    }
    let _commit = crate::basis::commit();
    let mut records = ORGANIZATIONS.lock();
    let at = records
        .iter()
        .position(|r| Arc::ptr_eq(&r.nest, &nest))
        .ok_or(NestFail::Denied)?;
    nest.close_external().map_err(|_| NestFail::Denied)?;
    if records[at].binding.is_some() || !super::external::discard(&nest) {
        return Err(NestFail::Busy);
    }
    nest.delete_organization().map_err(|_| NestFail::Busy)?;
    records.remove(at);
    Ok(())
}

pub(crate) fn maintain() -> bool {
    use core::sync::atomic::{AtomicUsize, Ordering};
    static CURSOR: AtomicUsize = AtomicUsize::new(0);
    for _ in 0..4 {
        let mut notice = None;
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
        let invalid = record.root.upgrade().is_none_or(|root| !root.alive());
        if invalid {
            let _ = record.nest.close_external();
        }
        if record.binding.is_some() {
            if before.0 != NestPhase::Closed {
                for _ in 0..4 {
                    match record.nest.external_fault() {
                        Ok(Some(fault)) => {
                            // A hardware fault has no Memo generation. Retain it
                            // at binding scope without guessing a publication.
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
            }
            if record.nest.progress_external_close().unwrap_or(false) {
                if let Some(root) = record.binding.take() {
                    // SAFETY: Nest observed the exact complete binding proof,
                    // including all payloads, queues, tables and platform ordering.
                    unsafe { root.release_retirement() };
                    notice = record.notification.as_ref().and_then(|(task, entry)| {
                        entry.upgrade().map(|entry| (*task, entry.token()))
                    });
                }
            }
        }
        changed = before != (record.nest.external_phase(), record.fault_sequence);
        if invalid
            && record.binding.is_none()
            && record.nest.external_settled()
            && super::external::discard(&record.nest)
            && record.nest.delete_organization().is_ok()
        {
            records.remove(at);
        }
        drop(records);
        drop(_commit);
        if let Some((task, token)) = notice {
            pend::notify(task, token);
        }
        if changed {
            crate::basis::sign::signal(crate::basis::sign::WakeKey::ExternalNest { id });
        }
    }
    ORGANIZATIONS.lock().iter().any(|r| r.binding.is_some())
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
