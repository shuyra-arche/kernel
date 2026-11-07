//! Boot is the only mint for administrative qualifications.
use crate::core::work::unit::task::Task;
use kernel_api::{
    abi::call::pend::PendFail,
    boot::ledger::ResourceKind,
    boot::ledger::{Call, Entry, Name},
    model::unit::ControlKind,
};
pub(crate) const ENTRIES: usize = 5;
pub(crate) fn authority(target: &Task) -> Result<[Entry; ENTRIES], PendFail> {
    let _commit = crate::core::work::commit();
    let mut entries = [Entry::NONE; ENTRIES];
    for (slot, (call, kind, delegable)) in entries.iter_mut().zip([
        (Call::Build, ControlKind::Build, true),
        (Call::Doom, ControlKind::Doom, true),
        (Call::Device, ControlKind::Device, true),
        (Call::Retain, ControlKind::Retain, true),
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
