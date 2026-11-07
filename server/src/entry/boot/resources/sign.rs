use crate::basis::work::{room::interrupt, unit::task::Task};
use kernel_api::{
    abi::call::pend::PendFail,
    boot::ledger::ResourceKind,
    boot::ledger::{Entry, Name, Trap},
    model::sign::SignRights,
};
pub(crate) const ENTRIES: usize = 1;
pub(crate) fn authority(target: &Task) -> Result<Entry, PendFail> {
    let _commit = crate::basis::work::commit();
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
