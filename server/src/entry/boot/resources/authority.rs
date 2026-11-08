//! Boot-only grants issued when the entry task is created.
use crate::basis::room::interrupt;
use crate::basis::unit::task::Task;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::boot::ledger::Call;
use kernel_api::boot::ledger::Entry;
use kernel_api::boot::ledger::Name;
use kernel_api::boot::ledger::{ResourceKind, Trap};
use kernel_api::model::sign::SignRights;
use kernel_api::model::unit::ControlKind;
pub(crate) const CONTROL_ENTRIES: usize = 2;
pub(crate) fn grant_controls(target: &Task) -> Result<[Entry; CONTROL_ENTRIES], PendFail> {
    let _commit = crate::basis::commit();
    let mut entries = [Entry::NONE; CONTROL_ENTRIES];
    for (slot, (call, kind, delegable)) in entries.iter_mut().zip([
        (Call::Build, ControlKind::Build, true),
        (Call::Guard, ControlKind::Guard, false),
    ]) {
        let id = target
            .administration
            .install(kind, delegable)
            .map_err(|_| PendFail::OoM)?;
        *slot = Entry::new(Name::Call(call), ResourceKind::Control, id.get());
    }
    Ok(entries)
}

pub(crate) const SIGN_ENTRIES: usize = 1;
pub(crate) fn grant_sign(target: &Task) -> Result<Entry, PendFail> {
    let _commit = crate::basis::commit();
    let id = target
        .signs
        .install(
            interrupt::get().supervisor_external.clone(),
            SignRights::LISTEN,
            false,
        )
        .map_err(|_| PendFail::OoM)?;
    Ok(Entry::new(
        Name::Trap(Trap::SupervisorExternal),
        ResourceKind::Sign,
        id.get(),
    ))
}
