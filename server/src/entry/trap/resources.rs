use crate::basis::pend;
use crate::basis::room::interrupt;
use crate::basis::view::View;
use crate::entry::boot::resources::Registry;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::boot::ledger::Name;
use kernel_api::boot::ledger::Trap;
use kernel_api::model::identity::Mark;
use kernel_api::model::modifiable::Modifiable;

pub(crate) fn register(registry: &mut Registry) -> Result<(), PendFail> {
    let resources = interrupt::get();
    let modifiable = Modifiable::No;
    let fault = pend::PendEntry::try_new(
        View::Clone(resources.page_fault.clone()),
        Mark::NONE,
        modifiable,
        None,
    )?;
    registry
        .register(Name::Trap(Trap::PageFault), fault)
        .map_err(|e| e.into_parts().0)?;
    Ok(())
}
