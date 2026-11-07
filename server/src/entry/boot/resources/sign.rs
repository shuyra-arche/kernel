use crate::basis::room::interrupt;
use crate::basis::unit::task::Task;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::boot::ledger::Entry;
use kernel_api::boot::ledger::Name;
use kernel_api::boot::ledger::ResourceKind;
use kernel_api::boot::ledger::Trap;
use kernel_api::model::sign::SignRights;
pub(crate) const ENTRIES: usize = 1;
pub(crate) fn authority(target: &Task) -> Result<Entry, PendFail> {
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
