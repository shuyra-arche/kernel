use crate::core::work::mail::Mail;
use crate::core::work::pend;
use crate::core::work::room::interrupt;
use crate::entry::boot::resources::Registry;
use kernel_api::{
    abi::call::pie::PieFail, boot::ledger::Name, boot::ledger::Trap, model::identity::Mark,
    model::permission::Permission,
};

pub(crate) fn register(registry: &mut Registry) -> Result<(), PieFail> {
    let resources = interrupt::get();
    let permission = Permission::FETCH | Permission::VEST;
    let external = pend::Pie::try_new(
        Mail::Nole(resources.supervisor_external.clone()),
        Mark::NONE,
        permission,
        None,
    )?;
    registry
        .register(Name::Trap(Trap::SupervisorExternal), external)
        .map_err(|e| e.into_parts().0)?;
    let fault = pend::Pie::try_new(
        Mail::Hole(resources.page_fault.clone()),
        Mark::NONE,
        permission,
        None,
    )?;
    registry
        .register(Name::Trap(Trap::PageFault), fault)
        .map_err(|e| e.into_parts().0)?;
    Ok(())
}
