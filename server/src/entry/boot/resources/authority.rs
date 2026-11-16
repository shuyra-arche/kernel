//! Boot-only grants issued when the entry task is created.
use crate::basis::unit::task::Task;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::boot::ledger::Call;
use kernel_api::boot::ledger::Entry;
use kernel_api::boot::ledger::Name;
use kernel_api::boot::ledger::ResourceKind;
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
            .map_err(|_| PendFail::OutOfMemory)?;
        *slot = Entry::new(Name::Call(call), ResourceKind::Control, id.get());
    }
    Ok(entries)
}

pub(crate) fn register_space(
    registry: &mut super::Registry,
    nest: alloc::sync::Arc<crate::basis::nest::Nest>,
) -> Result<(), PendFail> {
    use crate::basis::pend::{PendEntry, Target};
    use kernel_api::model::{
        identity::{Mark, TaskId},
        modifiable::Modifiable,
        pend::{Distributable, Limits, NestRights, NestScope, Operations, PendScope},
    };
    let root = PendEntry::try_new(
        Target::Nest {
            owner: TaskId::new(0),
            nest,
        },
        Mark::NONE,
        Modifiable::Yes,
        None,
    )?;
    root.narrow(Limits {
        operations: Operations::Nest(NestRights::INSPECT | NestRights::RESERVE),
        modifiable: Modifiable::Yes,
        distributable: Distributable::Yes,
        scope: PendScope::Nest(NestScope::Whole),
    })?;
    registry
        .register(Name::Space, root)
        .map_err(|failure| failure.into_parts().0)
}

pub(crate) const CALL_ENTRIES: usize = 1;
pub(crate) fn grant_calls(target: &Task) -> Result<Entry, PendFail> {
    use crate::basis::pend::{self, PendEntry, Target};
    use kernel_api::model::{identity::Mark, modifiable::Modifiable};
    let entry = PendEntry::try_new(
        Target::None {
            owner: target.ident.id,
        },
        Mark::NONE,
        Modifiable::No,
        None,
    )?;
    let token = entry.token();
    pend::insert(target, entry)?;
    Ok(Entry::new(
        Name::Call(Call::Create),
        ResourceKind::None,
        token.get(),
    ))
}
